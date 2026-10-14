# Copyright 2026 Nowledge
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Optional IPython cell magics: `%%cypher`, `%%sql` and the `%hawdb` line magic.

Load with `%load_ext hawdb.magic` or `import hawdb.magic`. Importing registers
the magics only when an IPython shell is already running; IPython is never
imported by this module.

The database is the one chosen with `%hawdb`, otherwise the `hawdb_db`
variable in the notebook namespace; either must be a `hawdb.Database`.
Parameters come from the `params` variable in the notebook namespace: a dict
for `%%cypher`, a list for `%%sql`. It is read by every cell, so set
`params = None` once a query no longer needs it. The magic line itself takes
no options, and any text on it is an error. The cell body is sent to the
engine unchanged and never parsed.

Line magic:

    %hawdb open [PATH]   open PATH (in-memory when omitted) and use it;
                         PATH may be wrapped in one pair of quotes
    %hawdb use NAME      use the Database held in notebook variable NAME
    %hawdb close         forget the selected database

`open` makes HawDB own the handle, so `close` (and `%unload_ext hawdb.magic`)
closes it. A handle chosen with `use`, or found as `hawdb_db`, belongs to the
notebook: `close` only forgets a `use` selection, and a `hawdb_db` handle must
be closed by the caller. `%unload_ext hawdb.magic` removes the magics.
"""

import sys

import hawdb
from hawdb import _display

_selected = None
_owned = False  # whether `_selected` was opened by `%hawdb open`
_shell = None


def _namespace():
    return _shell.user_ns if _shell is not None else {}


def _database():
    if _selected is not None:
        return _selected
    db = _namespace().get("hawdb_db")
    if db is None:
        raise RuntimeError(
            "no HawDB database selected: run '%hawdb open [PATH]' or "
            "'%hawdb use NAME', or set a 'hawdb_db' variable"
        )
    if not isinstance(db, hawdb.Database):
        raise TypeError(
            f"'hawdb_db' must be a hawdb.Database, got {type(db).__name__}"
        )
    return db


def _params(magic, line, expected):
    """Return the notebook's `params` after checking the magic line is empty."""
    if line.strip():
        raise ValueError(f"{magic} takes no options; set a 'params' variable instead")
    params = _namespace().get("params")
    if params is not None and not isinstance(params, expected):
        raise TypeError(
            f"{magic} expects params to be a {expected.__name__}, got "
            f"{type(params).__name__} (set params = None to clear it)"
        )
    return params


def _forget():
    """Drop the selection, closing the handle only when `%hawdb open` made it."""
    global _selected, _owned
    if _selected is not None and _owned:
        _selected.close()
    _selected = None
    _owned = False


def _unquote(argument):
    """Strip one pair of matching outer quotes, leaving backslashes alone.

    `shlex` is avoided because it would mangle Windows paths.
    """
    if len(argument) >= 2 and argument[0] == argument[-1] and argument[0] in "'\"":
        return argument[1:-1]
    return argument


_LINE_USAGE = "usage: %hawdb open [PATH] | use NAME | close"


def hawdb_line(line):
    """Implementation of the `%hawdb` line magic."""
    global _selected, _owned
    action, _, argument = line.strip().partition(" ")
    argument = argument.strip()
    if action == "open":
        # Open first: a failed open keeps the current selection usable.
        db = hawdb.open(_unquote(argument) or None)
        _forget()
        _selected = db
        _owned = True
        return None
    if action == "use":
        if not argument or len(argument.split()) != 1:
            raise ValueError(_LINE_USAGE)
        db = _namespace().get(argument)
        if not isinstance(db, hawdb.Database):
            raise ValueError(f"{argument!r} is not a hawdb.Database in this notebook")
        _forget()
        _selected = db
        return None
    if action == "close":
        if argument:
            raise ValueError(_LINE_USAGE)
        if _selected is None and _namespace().get("hawdb_db") is not None:
            raise ValueError(
                "'hawdb_db' belongs to the notebook: close it with "
                "hawdb_db.close() and delete or reassign the variable"
            )
        _forget()
        return None
    raise ValueError(_LINE_USAGE)


# Rows fetched per call while a graph display scans past its window.
_SCAN_BATCH_ROWS = 1000


class CypherResult:
    """What `%%cypher` returns: its `QueryResult`, drawn as a graph when possible.

    Attribute access, iteration, `len` and the text `repr` go to the wrapped
    `QueryResult`, which `result` exposes. In the notebook, the nodes and
    relationships in the displayed rows are drawn above the usual table.
    """

    def __init__(self, result):
        self.result = result

    def __getattr__(self, name):
        if name == "result":  # not set yet, e.g. during `copy.copy`
            raise AttributeError(name)
        return getattr(self.result, name)

    def __iter__(self):
        return self

    def __next__(self):
        return next(self.result)

    def __len__(self):
        return len(self.result)

    def __repr__(self):
        return repr(self.result)

    def _repr_html_(self):
        limit = _display.DISPLAY_ROW_LIMIT
        rows = self.result._display_rows(0, limit)
        table = _display.render_html(self.result.columns, rows, len(self.result), limit)
        graph = _display.render_graph(
            rows,
            self._rows_after(limit),
            limit=limit,
            scan_limit=_display.GRAPH_SCAN_ROW_LIMIT,
        )
        return graph + table

    def _rows_after(self, start):
        # Lazy and batched: rows past the window are fetched only when the
        # window holds something to draw, and only as far as the scan reads.
        while batch := self.result._display_rows(start, start + _SCAN_BATCH_ROWS):
            yield from batch
            start += len(batch)


def cypher_cell(line, cell):
    """Implementation of `%%cypher`: run the cell as one Cypher statement."""
    params = _params("%%cypher", line, dict)
    return CypherResult(_database().execute(cell, params))


def sql_cell(line, cell):
    """Implementation of `%%sql`: run the cell as one SQL statement."""
    params = _params("%%sql", line, list)
    return _database().execute_sql(cell, params)


# Every magic this module installs; `register` and `unregister` both read it.
_MAGICS = (
    (hawdb_line, "line", "hawdb"),
    (cypher_cell, "cell", "cypher"),
    (sql_cell, "cell", "sql"),
)


def register(shell):
    """Register the magics on an IPython shell."""
    global _shell
    if _shell is shell:
        return  # importing already registered; `%load_ext` calls us again
    _shell = shell
    for func, kind, name in _MAGICS:
        shell.register_magic_function(func, kind, name)


def unregister(shell):
    """Remove the magics and release the selected database."""
    global _shell
    _forget()
    _shell = None
    registered = shell.magics_manager.magics
    for _, kind, name in _MAGICS:
        registered[kind].pop(name, None)


def load_ipython_extension(shell):
    register(shell)


def unload_ipython_extension(shell):
    unregister(shell)


def _running_shell():
    ipython = sys.modules.get("IPython")
    return ipython.get_ipython() if ipython is not None else None


_running = _running_shell()
if _running is not None:
    register(_running)
