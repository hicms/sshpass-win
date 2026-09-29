# sshpass

Non-interactive ssh password authentication for Windows.

`sshpass` runs a command inside a Windows pseudo console (ConPTY), waits for
the password prompt and types the password for you.

## Requirements

Windows 10 version 1809 (build 17763) or later, which provides ConPTY. The
binary is statically linked and needs no Visual C++ runtime.

## Build

```powershell
.\scripts\build.ps1
```

This runs rustfmt, clippy and the tests, then builds the release executable
and copies it to `dist\sshpass.exe` with a `dist\sshpass.exe.sha256` checksum.
Add `-SkipChecks` to build without the checks, or run
`cargo build --release` directly (`target\release\sshpass.exe`).

## Usage

```text
Usage: sshpass [-f|-d|-p|-e] [-hvk] [-P prompt] command arguments
   -f filename   Take password to use from file
   -d number     Use number as file handle for getting password
   -p password   Provide password as argument (security unwise)
   -e            Password is passed as env-var "SSHPASS"
   With no parameters - password will be taken from stdin

   -P prompt     Which string should sshpass search for to detect a password prompt
                 (default "password:")
   -v            Be verbose about what you're doing
   -k, --auto-confirm
                 Auto confirm 'Are you sure...' prompt for host keys
   -h, --help    Show help (this screen)
```

At most one password source may be given. Options stop at the first argument
that is not an option; everything after it belongs to the command. Use `--` to
end options explicitly.

Prefer `-e`, `-f` or standard input over `-p`: a password on the command line
is visible to other processes.

## Examples

```sh
sshpass -p 12345 ssh user@192.168.139.128 ls
```

```sh
set SSHPASS=12345
sshpass -e ssh -o StrictHostKeyChecking=ask user@192.168.139.128 uptime
```

```sh
sshpass -p 12345 rsync -avz -e "c:/path/to/ssh.exe" README.md user@192.168.139.128:/home/user/
```

## Exit codes

| Code | Meaning                                                    |
| ---- | ---------------------------------------------------------- |
| 0..  | The command's own exit code, passed through unchanged      |
| 1    | Invalid arguments                                          |
| 2    | Conflicting password sources                               |
| 3    | Runtime error (password unreadable, command failed to start, ...) |
| 5    | Incorrect password                                         |
| 6    | Host public key is unknown (use `-k` to accept it)         |
| 7    | Host public key for the IP address has changed             |

## Behavior

- Arguments reach the command intact: spaces, quotes and backslashes are
  quoted according to the Windows command-line rules.
- The command's exit code is returned, and sshpass stops the command when it
  detects a wrong password or an unconfirmed host key instead of hanging.
- Prompts split across output chunks are still detected.
- The password is read before the command starts; a missing password file or
  `SSHPASS` variable is reported without connecting.
- `-v` messages go to standard error with an `SSHPASS:` prefix.
- Console modes are restored on exit and the console code page is left alone.
- Output that only exists because of the pseudo console (the echoed Enter after
  the password, ConPTY's mode switches) is not passed on.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The end-to-end tests run the real `sshpass.exe` against a fake `ssh` (the test
executable itself) inside a pseudo console; they need no network.

## Releasing

```powershell
.\scripts\release.ps1 -Version 0.2.0 -DryRun   # check the preconditions only
.\scripts\release.ps1 -Version 0.2.0
```

The script requires a clean, up-to-date `main`, sets the version in
`Cargo.toml`, runs `scripts\build.ps1`, commits `Release v0.2.0`, creates the tag
`v0.2.0` and pushes both. Pushing the tag starts the GitHub Actions workflow,
which builds the executable and publishes it, with its checksum, as a GitHub
release.

## License

Apache-2.0. See [LICENSE](LICENSE).
