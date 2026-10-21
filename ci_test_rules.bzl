"""Optimized execution for expensive CI fixtures, with debug checks retained."""

load("@rules_rust//rust:defs.bzl", "rust_test")

_EXTRA_FLAGS = "@rules_rust//rust/settings:extra_rustc_flags"
_CHECKED_FLAGS = [
    "-Copt-level=2",
    "-Cdebug-assertions=yes",
    "-Coverflow-checks=yes",
]

def _checked_optimization_impl(settings, _attr):
    flags = [flag for flag in settings[_EXTRA_FLAGS] if flag not in _CHECKED_FLAGS]
    return {_EXTRA_FLAGS: flags + _CHECKED_FLAGS}

_checked_optimization = transition(
    implementation = _checked_optimization_impl,
    inputs = [_EXTRA_FLAGS],
    outputs = [_EXTRA_FLAGS],
)

def _forward_executable(ctx):
    binary = ctx.attr.binary[0][DefaultInfo]
    name = ctx.attr.executable_name or ctx.label.name
    executable = ctx.actions.declare_file(ctx.label.name + "/" + name)
    ctx.actions.symlink(
        output = executable,
        target_file = binary.files_to_run.executable,
        is_executable = True,
    )
    return [DefaultInfo(executable = executable, runfiles = binary.default_runfiles)]

def _attributes():
    return {
        "binary": attr.label(executable = True, cfg = _checked_optimization),
        "executable_name": attr.string(),
        "_allowlist_function_transition": attr.label(
            default = "@bazel_tools//tools/allowlists/function_transition_allowlist",
        ),
    }

checked_ci_binary = rule(
    implementation = _forward_executable,
    executable = True,
    attrs = _attributes(),
)

_checked_ci_test = rule(
    implementation = _forward_executable,
    test = True,
    attrs = _attributes(),
)

def checked_rust_test(name, size = "medium", timeout = "long", args = [], tags = [], **kwargs):
    """Optimize the entire target dependency closure, preserving public test args."""
    binary = name + "_ci_binary"
    rust_test(
        name = binary,
        size = size,
        timeout = timeout,
        tags = ["manual"],
        visibility = ["//visibility:private"],
        **kwargs
    )
    _checked_ci_test(
        name = name,
        binary = ":" + binary,
        args = args,
        size = size,
        timeout = timeout,
        tags = tags,
    )
