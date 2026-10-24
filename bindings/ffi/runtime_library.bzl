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

"""Expose the shared object without its platform linker-only companion files."""

load("@rules_rust//rust:rust_common.bzl", "TestCrateInfo")

def _rust_runtime_library_impl(ctx):
    library = ctx.attr.library

    # rules_rust wraps cdylib CrateInfo in its public TestCrateInfo provider.
    # DefaultInfo also contains the .dll.lib import library on Windows.
    crate = library[TestCrateInfo].crate
    if crate.type != "cdylib":
        fail("runtime library requires a Rust cdylib, got " + crate.type)
    return [DefaultInfo(
        files = depset([crate.output]),
        runfiles = ctx.runfiles(files = [crate.output]).merge(library[DefaultInfo].default_runfiles),
    )]

rust_runtime_library = rule(
    implementation = _rust_runtime_library_impl,
    attrs = {
        "library": attr.label(mandatory = True, providers = [TestCrateInfo]),
    },
)
