// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

// Internal manual Bazel fixture. The normal comparison CLI calls Embedded
// directly in-process and does not launch this executable.
mod embedded;

use std::io::{self, BufRead, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut database = embedded::Embedded::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.len() > 1_048_576 {
            return Err("request exceeds 1 MiB".into());
        }
        let request = serde_json::from_str(&line)?;
        serde_json::to_writer(&mut stdout, &database.request(request))?;
        writeln!(stdout)?;
        stdout.flush()?;
    }
    Ok(())
}
