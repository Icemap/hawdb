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

"""HTML table and graph rendering for notebook display (no IPython needed)."""

import itertools
import math
import unicodedata
from html import escape


def _cell(value):
    if value is None:
        return "NULL"
    if isinstance(value, (bytes, bytearray)):
        return repr(value)
    return str(value)


# Rows a notebook table renders before noting how many were omitted.
DISPLAY_ROW_LIMIT = 100


def render_html(columns, rows, total=None, limit=DISPLAY_ROW_LIMIT):
    """Render `rows` (dicts keyed by column) as an escaped HTML table.

    `rows` may already be cut down to the first `limit` rows, with `total`
    giving the full count; it defaults to `len(rows)`. At most `limit` rows are
    drawn; when more exist a note says how many were omitted.
    """
    if total is None:
        total = len(rows)
    if total == 0 and not columns:
        return "<p>(no rows)</p>"
    rows = rows[:limit]
    parts = ["<table>", "<thead><tr>"]
    parts.extend(f"<th>{escape(str(column))}</th>" for column in columns)
    parts.append("</tr></thead><tbody>")
    for row in rows:
        parts.append("<tr>")
        parts.extend(
            f"<td>{escape(_cell(row.get(column)))}</td>" for column in columns
        )
        parts.append("</tr>")
    parts.append("</tbody></table>")
    if total == 0:
        parts.append("<p>(no rows)</p>")
    omitted = total - len(rows)
    if omitted > 0:
        parts.append(
            f"<p>{omitted} more row{'s' if omitted != 1 else ''} not shown "
            f"({total} total).</p>"
        )
    return "".join(parts)


# Graph drawing for `%%cypher`. Recognition reads the maps the executor builds
# for a returned node or relationship; the statement text is never parsed.

# Rows past the drawn window that a graph display scans to count what it left
# out; beyond this budget the note gives a lower bound.
GRAPH_SCAN_ROW_LIMIT = 10_000

# Text gets a halo in the page background so lines crossing it stay readable;
# JupyterLab and VS Code expose their background, other hosts fall back to white.
_LABEL_STYLE = (
    "<style>.hawdb-label{fill:currentColor;paint-order:stroke;stroke-width:3px;"
    "stroke-linejoin:round;stroke:var(--jp-layout-color0,"
    "var(--vscode-editor-background,#fff))}</style>"
)

_NODE_TEXT_KEYS = ("name", "title", "code")
_TEXT_LIMIT = 18
_NODE_RADIUS = 18
_MARGIN = 60
_FONT_SIZE = 11
_PALETTE = (
    "#4e79a7",
    "#f28e2b",
    "#59a14f",
    "#e15759",
    "#76b7b2",
    "#edc948",
    "#b07aa1",
    "#ff9da7",
    "#9c755f",
    "#bab0ac",
)


def _is_int(value):
    return type(value) is int  # `bool` is an `int` but never an id


def is_relationship(value):
    """A relationship map: integer `_id`, `source_id` and `target_id`, and `type`.

    Checked before `is_node`: a relationship may carry a `labels` property, and
    its full endpoint structure is the stronger signal.
    """
    return (
        isinstance(value, dict)
        and _is_int(value.get("_id"))
        and _is_int(value.get("source_id"))
        and _is_int(value.get("target_id"))
        and "type" in value
    )


def is_node(value):
    """A node map: integer `_id` and a list `labels`, and not a relationship."""
    return (
        isinstance(value, dict)
        and _is_int(value.get("_id"))
        and isinstance(value.get("labels"), list)
        and not is_relationship(value)
    )


def _collect(rows):
    """Return `(nodes, relationships)` in `rows`, each keyed by `_id`.

    Only top-level cells count; the first map seen for an `_id` is kept.
    """
    nodes, relationships = {}, {}
    for row in rows:
        for value in row.values():
            if is_relationship(value):
                relationships.setdefault(value["_id"], value)
            elif is_node(value):
                nodes.setdefault(value["_id"], value)
    return nodes, relationships


def _short(text):
    return text if len(text) <= _TEXT_LIMIT else text[: _TEXT_LIMIT - 1] + "…"


def _first_label(node):
    return node["labels"][0] if node["labels"] else ""


def _node_text(node):
    for key in _NODE_TEXT_KEYS:
        if node.get(key) is not None:
            return _cell(node[key])
    return str(node["_id"])


def _tooltip(element):
    return ", ".join(f"{key}: {_cell(value)}" for key, value in element.items())


def _plural(count, noun):
    return f"{count} {noun}{'s' if count != 1 else ''}"


def _text_width(text):
    """Generous rendered width: wide (e.g. CJK) characters a full em, others 0.8."""
    return _FONT_SIZE * sum(
        1.0 if unicodedata.east_asian_width(char) in "WF" else 0.8 for char in text
    )


def _layout(count, side_margin):
    """Deterministic circle placement.

    Returns `(width, height, middle, [(x, y), ...])`, `middle` being the center
    of the circle; `side_margin` is the room left and right of it.
    """
    radius = 0.0 if count == 1 else max(130.0, count * 3 * _NODE_RADIUS / math.tau)
    width, height = 2 * (radius + side_margin), 2 * (radius + _MARGIN)
    middle = (width / 2, height / 2)
    points = [
        (
            middle[0] + radius * math.cos(math.tau * i / count - math.pi / 2),
            middle[1] + radius * math.sin(math.tau * i / count - math.pi / 2),
        )
        for i in range(count)
    ]
    return width, height, middle, points


def _edge_path(start, end, bend):
    """Path from `start` to `end`, trimmed to both circles and bent by `bend`.

    Returns the SVG path and the label position on the curve.
    """
    (x1, y1), (x2, y2) = start, end
    length = math.hypot(x2 - x1, y2 - y1) or 1.0
    ux, uy = (x2 - x1) / length, (y2 - y1) / length
    cx = (x1 + x2) / 2 - uy * bend  # control point, off the midpoint
    cy = (y1 + y2) / 2 + ux * bend
    sx, sy = x1 + ux * _NODE_RADIUS, y1 + uy * _NODE_RADIUS
    ex, ey = x2 - ux * (_NODE_RADIUS + 3), y2 - uy * (_NODE_RADIUS + 3)
    label = ((sx + 2 * cx + ex) / 4, (sy + 2 * cy + ey) / 4)
    return f"M{sx:.1f},{sy:.1f} Q{cx:.1f},{cy:.1f} {ex:.1f},{ey:.1f}", label


def _loop_path(center, index):
    """A self-loop above the node; later loops on the same node grow taller."""
    x, y = center
    r = _NODE_RADIUS
    height = r * (3 + index)
    path = (
        f"M{x - r * 0.5:.1f},{y - r * 0.87:.1f} "
        f"C{x - r * 1.6:.1f},{y - height:.1f} {x + r * 1.6:.1f},{y - height:.1f} "
        f"{x + r * 0.5:.1f},{y - r * 0.87 - 3:.1f}"
    )
    return path, (x, y - height * 0.8)


def render_graph(
    rows, rest=(), limit=DISPLAY_ROW_LIMIT, scan_limit=GRAPH_SCAN_ROW_LIMIT
):
    """Draw the nodes and relationships in `rows` as escaped inline SVG.

    Only top-level cells that are node or relationship maps are drawn; any
    other cell is a column value. Nodes and relationships are deduplicated by
    `_id`. A relationship whose endpoint was not returned still points at a
    dashed placeholder labeled with that `source_id` or `target_id`.

    At most `limit` rows are drawn. Rows past it, then the iterable `rest`, are
    only scanned to note how many nodes and relationships were not drawn, and
    only when something is drawn. At most `scan_limit` of those rows are read;
    when more remain, the note counts are a lower bound. Returns `""` when the
    drawn rows contain no node or relationship.
    """
    rest = itertools.chain(rows[limit:], rest)
    nodes, relationships = _collect(rows[:limit])
    if not nodes and not relationships:
        return ""

    later_nodes, later_relationships = _collect(itertools.islice(rest, scan_limit))
    note = _omission_note(
        hidden_nodes=len(later_nodes.keys() - nodes.keys()),
        hidden_relationships=len(later_relationships.keys() - relationships.keys()),
        limit=limit,
        unchecked_after=limit + scan_limit if next(rest, None) is not None else None,
    )
    return _svg(nodes, relationships) + note


def _svg(nodes, relationships):
    # Endpoints that were not returned are placed after the returned nodes.
    missing = []
    for relationship in relationships.values():
        for endpoint in (relationship["source_id"], relationship["target_id"]):
            if endpoint not in nodes and endpoint not in missing:
                missing.append(endpoint)
    texts = {node_id: _short(_node_text(node)) for node_id, node in nodes.items()}
    texts.update((node_id, str(node_id)) for node_id in missing)
    # Text sits beside the leftmost and rightmost nodes, so leave room for it.
    widest = max(_text_width(text) for text in texts.values())
    side_margin = max(_MARGIN, _NODE_RADIUS + 6 + widest + 8)
    width, height, middle, points = _layout(len(texts), side_margin)
    position = dict(zip([*nodes, *missing], points))

    colors = {}
    for node in nodes.values():
        colors.setdefault(_first_label(node), _PALETTE[len(colors) % len(_PALETTE)])

    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0f}" '
        f'height="{height:.0f}" viewBox="0 0 {width:.0f} {height:.0f}" '
        f'font-family="sans-serif" font-size="{_FONT_SIZE}" role="img">',
        _LABEL_STYLE,
        '<defs><marker id="hawdb-arrow" viewBox="0 0 10 10" refX="9" refY="5" '
        'markerWidth="7" markerHeight="7" orient="auto">'
        '<path d="M0,0 L10,5 L0,10 z" fill="#888"/></marker></defs>',
    ]

    # Relationships between the same pair of nodes bend apart, alternating sides.
    drawn_between = {}
    for relationship in relationships.values():
        source, target = relationship["source_id"], relationship["target_id"]
        pair = (min(source, target), max(source, target))
        index = drawn_between.get(pair, 0)
        drawn_between[pair] = index + 1
        if source == target:
            path, label_at = _loop_path(position[source], index)
        else:
            bend = (index + 1) // 2 * 44 * (1 if index % 2 else -1)
            if source > target:
                bend = -bend  # the same side for either direction
            path, label_at = _edge_path(position[source], position[target], bend)
        parts.append(_relationship_svg(relationship, path, label_at))

    for node_id, node in nodes.items():
        center = position[node_id]
        style = f'fill="{colors[_first_label(node)]}" stroke="#555"'
        label = _node_label_at(center, middle)
        parts.append(_vertex_svg(center, label, style, _tooltip(node), texts[node_id]))
    for node_id in missing:
        center = position[node_id]
        style = 'fill="none" stroke="#888" stroke-dasharray="4 3"'
        tooltip = f"_id: {node_id} (not returned)"
        label = _node_label_at(center, middle)
        parts.append(_vertex_svg(center, label, style, tooltip, texts[node_id]))
    parts.append("</svg>")
    return "".join(parts)


def _relationship_svg(relationship, path, label_at):
    x, y = label_at
    text = _short(_cell(relationship["type"]))
    return (
        f"<g><title>{escape(_tooltip(relationship))}</title>"
        f'<path d="{path}" fill="none" stroke="#888" stroke-width="1.5" '
        'marker-end="url(#hawdb-arrow)"/>'
        f'<text x="{x:.1f}" y="{y:.1f}" text-anchor="middle" '
        f'class="hawdb-label">{escape(text)}</text></g>'
    )


def _node_label_at(center, middle):
    """Place a node's text on its side facing away from the layout middle.

    Relationships mostly cross the inside of the circle, so the outer side
    keeps text off them. Returns the text position and its `text-anchor`.
    """
    (x, y), (mx, my) = center, middle
    length = math.hypot(x - mx, y - my)
    if length < 1:
        return (x, y + _NODE_RADIUS + 13), "middle"  # a lone node: below it
    ux, uy = (x - mx) / length, (y - my) / length
    anchor = "start" if ux > 0.3 else "end" if ux < -0.3 else "middle"
    distance = _NODE_RADIUS + 6
    return (x + ux * distance, y + uy * (distance + 4) + 4), anchor


def _vertex_svg(center, label, style, tooltip, text):
    x, y = center
    (lx, ly), anchor = label
    return (
        f"<g><title>{escape(tooltip)}</title>"
        f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{_NODE_RADIUS}" {style}/>'
        f'<text x="{lx:.1f}" y="{ly:.1f}" text-anchor="{anchor}" '
        f'class="hawdb-label">{escape(text)}</text></g>'
    )


def _omission_note(hidden_nodes, hidden_relationships, limit, unchecked_after):
    """Say what was left out; `unchecked_after` is set when the scan stopped early."""
    hidden = []
    if hidden_nodes:
        hidden.append(_plural(hidden_nodes, "node"))
    if hidden_relationships:
        hidden.append(_plural(hidden_relationships, "relationship"))
    parts = []
    if hidden:
        bound = "At least " if unchecked_after is not None else ""
        parts.append(
            f"<p>{bound}{' and '.join(hidden)} not drawn "
            f"(beyond the first {limit} rows).</p>"
        )
    if unchecked_after is not None:
        parts.append(
            f"<p>Rows after the first {unchecked_after} were not checked for "
            "nodes or relationships.</p>"
        )
    return "".join(parts)
