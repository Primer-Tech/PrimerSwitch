//! Hermetic diagnostics: never resolves the real home or creates a network transport.
mod fixtures;

use serde_json::json;
use std::process::ExitCode;
use switcher_runtime::RuntimeHandle;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Fixtures,
    Demo,
    Help,
}

fn parse(arguments: &[std::ffi::OsString]) -> Result<Mode, ()> {
    match arguments {
        [] => Ok(Mode::Help),
        [argument] if argument == "--fixtures" => Ok(Mode::Fixtures),
        [argument] if argument == "--demo-snapshot" => Ok(Mode::Demo),
        [argument] if argument == "--help" => Ok(Mode::Help),
        _ => Err(()),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let result = match parse(&arguments) {
        Ok(Mode::Help) => {
            println!(
                "PrimerSwitch hermetic diagnostics\n\nUsage: switcher-diagnostics [--fixtures | --demo-snapshot | --help]\n\n--fixtures       Verify encrypted storage, tamper preservation, JSON patches and\n                 fake provider/runtime behavior in a disposable fixture home.\n--demo-snapshot  Print a redacted, read-only runtime demonstration snapshot.\n\nThese modes use no real account files, OS credential items or provider network."
            );
            return ExitCode::SUCCESS;
        }
        Ok(Mode::Demo) => serde_json::to_value(RuntimeHandle::demo().get_snapshot())
            .map_err(|_| "The demonstration snapshot could not be serialized"),
        Ok(Mode::Fixtures) => fixtures::run().await,
        Err(()) => {
            println!(
                "{}",
                json!({"ok":false,"error":"Unsupported arguments; use --help"})
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!("{}", json!({"mode":"fixtures","ok":false,"error":error}));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicit_hermetic_modes_are_accepted() {
        for argument in [
            "--home",
            "--live",
            "--network",
            "--export",
            "--token",
            "SENTINEL_SECRET",
        ] {
            assert!(parse(&[argument.into()]).is_err());
        }
        assert!(parse(&["--fixtures".into(), "--live".into()]).is_err());
        assert_eq!(parse(&["--fixtures".into()]), Ok(Mode::Fixtures));
        assert_eq!(parse(&["--demo-snapshot".into()]), Ok(Mode::Demo));
    }
}
