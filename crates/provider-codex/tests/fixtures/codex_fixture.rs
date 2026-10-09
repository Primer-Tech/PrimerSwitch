//! Fake-only subprocess, compiled only by the explicit fixture-process feature.
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};
fn emit(value: Value) {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &value).unwrap();
    writeln!(output).unwrap();
    output.flush().unwrap();
}
fn main() {
    let home =
        PathBuf::from(std::env::var_os("CODEX_HOME").expect("isolated fixture context required"));
    let marker = std::fs::read_to_string(home.join(".primerswitch-codex-fixture"))
        .expect("fixture marker required");
    let mode = marker
        .strip_prefix("PrimerSwitch fake Codex child v1\n")
        .expect("fixture marker mismatch")
        .trim();
    if std::env::args().nth(1).as_deref() == Some("--version") {
        match mode {
            "old-version" => println!("codex-cli 0.159.0"),
            "version-overflow" => println!("{}", "x".repeat(8192)),
            _ => println!("codex-cli 0.160.0"),
        };
        return;
    }
    assert_eq!(
        std::env::args().skip(1).collect::<Vec<_>>(),
        ["app-server", "--listen", "stdio://"]
    );
    eprintln!("SECRET-STDERR");
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let id = &request["id"];
        let method = request["method"].as_str().unwrap();
        let result = match method {
            "initialize" => {
                if mode == "frame-overflow" {
                    print!("{}", "x".repeat(1024 * 1024 + 1));
                    io::stdout().flush().unwrap();
                    continue;
                }
                let context = if mode == "wrong-home" {
                    home.join("wrong")
                } else {
                    home.clone()
                };
                json!({"codexHome":context,"platformOs":std::env::consts::OS,"platformFamily":if cfg!(windows){"windows"}else{"unix"},"userAgent":"fixture"})
            }
            "initialized" => continue,
            "account/login/start" => {
                if mode != "cancel" {
                    emit(
                        json!({"method":"account/login/completed","params":{"loginId":"fixture-login","success":true,"error":null}}),
                    );
                }
                json!({"type":"chatgpt","loginId":"fixture-login","authUrl":"https://auth.openai.com/oauth/authorize?state=fixture"})
            }
            "account/login/cancel" => json!({"status":"canceled"}),
            "account/read" => {
                assert_eq!(request["params"]["refreshToken"], false);
                json!({"account":{"type":"chatgpt","email":"fixture@example.invalid","planType":"plus"},"requiresOpenaiAuth":true})
            }
            "account/rateLimits/read" => {
                assert_eq!(request["params"]["supportsLunaReserve"], false);
                assert_eq!(request["params"]["excludeResetCreditDetails"], false);
                json!({"accountId":"fixture-workspace","ordinaryUsageAllowed":true,"rateLimits":{"limitId":"codex","primary":{"usedPercent":12,"windowDurationMins":300,"resetsAt":null}},"rateLimitsByLimitId":{"codex":{"limitId":"codex","secondary":null}}})
            }
            "config/read" => {
                assert_eq!(request["params"], json!({"includeLayers":true}));
                json!({"config":{"cli_auth_credentials_store":"file"},"origins":{},"layers":[]})
            }
            "configRequirements/read" => {
                assert!(request["params"].is_null());
                json!({"requirements":null})
            }
            "account/rateLimitResetCredit/consume" => {
                assert_eq!(
                    request["params"]["idempotencyKey"],
                    "native-fixture-reset-key"
                );
                json!({"outcome":"alreadyRedeemed"})
            }
            _ => panic!("unexpected fixture method"),
        };
        emit(json!({"id":id,"result":result}));
    }
    if mode == "shutdown-hang" {
        std::thread::sleep(std::time::Duration::from_secs(30));
        // This file would demonstrate that the bounded shutdown failed to kill us.
        std::fs::write(home.join("unexpected-survivor"), b"fixture-only").unwrap();
    }
}
