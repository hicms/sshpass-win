use std::{env, process};

fn main() {
    let code = sshpass::run(env::args_os().skip(1));
    // `ExitCode` only carries 8 bits, but Windows exit codes are 32 bits and
    // the command's code is passed on unchanged. Everything `run` owned has
    // been dropped by now, so skipping destructors loses nothing.
    process::exit(code.cast_signed());
}
