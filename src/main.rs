// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::thread;

use qs::cli;

/// Deeply nested values recurse in the merge and the serializer.
const STACK_BYTES: usize = 256 << 20;

fn serve() -> ExitCode {
    if std::env::args_os().len() > 1 {
        eprint!("{}", cli::USAGE);
        return ExitCode::from(2);
    }
    let mut input = Vec::new();
    if let Err(error) = io::stdin().read_to_end(&mut input) {
        eprintln!("qs: cannot read standard input: {error}");
        return ExitCode::from(1);
    }
    let (output, status) = cli::run(&input);
    let mut stdout = io::stdout().lock();
    if let Err(error) = stdout.write_all(&output).and_then(|()| stdout.flush()) {
        eprintln!("qs: cannot write standard output: {error}");
        return ExitCode::from(1);
    }
    ExitCode::from(status)
}

fn main() -> ExitCode {
    let worker = thread::Builder::new().stack_size(STACK_BYTES).spawn(serve);
    match worker.map(thread::JoinHandle::join) {
        Ok(Ok(status)) => status,
        Ok(Err(panic)) => {
            let reason = panic
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown cause");
            eprintln!("qs: internal error: {reason}");
            ExitCode::from(70)
        }
        Err(error) => {
            eprintln!("qs: cannot start: {error}");
            ExitCode::from(70)
        }
    }
}
