// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

mod compare;
mod embedded;
mod postgres;
mod regress;
mod suite;

fn main() {
    let code = match regress::Options::parse() {
        Ok(Some(options)) => regress::run(options),
        Ok(None) => 0,
        Err(error) => {
            eprintln!("INFRA_ERROR: {error}");
            2
        }
    };
    std::process::exit(code);
}
