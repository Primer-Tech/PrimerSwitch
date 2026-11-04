//! Offline integration fixtures for API-key activation evidence and consumption.
use super::*;
use switcher_platform::codex::usage::{
    ActivationSpan, KeyUsageLedger, UsageCounters, UsageLedger, UsageStore,
};
use switcher_platform::codex::{CodexSourceRole, CodexWatchedSource};

fn spans(f: &Fixture, id: &str) -> Vec<ActivationSpan> {
    f.engine.saved.accounts[f.engine.account_index(id).unwrap()]
        .usage_spans
        .clone()
}

fn scan(f: &mut Fixture, now: i64) {
    let job = f.engine.plan_key_usage(now).expect("usage scan pending");
    let result = job.run();
    assert!(result.is_ok(), "fixture rollout scan succeeds");
    assert!(
        !result.as_ref().unwrap().has_more(),
        "small fixture fits the byte cap"
    );
    f.engine.finish_key_usage(result);
}

#[tokio::test]
async fn an_expired_scan_reports_incomplete_zeroes_then_the_next_pass_counts_every_record() {
    // Reproduce scheduler exhaustion deterministically, without sleeping or
    // relying on this PC's current load to consume a 300 ms allowance.
    for (observation, now, timestamp) in [
        (200, 210, "1970-01-01T00:03:25Z"),
        (86_400, 40 * 86_400, "1970-02-09T12:00:00Z"),
    ] {
        let mut f = Fixture::new();
        f.add_key("Key", FAKE_KEY).await.unwrap();
        let key = f.key_id(FAKE_KEY);
        f.write_active(&key_file(FAKE_KEY));
        f.engine.observe(&f.vault, observation).unwrap();
        f.engine.observe(&f.vault, now).unwrap();
        rollout(&f, &usage_record("PRIVATE_RESPONSE_SENTINEL", timestamp));
        let result = f
            .engine
            .plan_key_usage(now)
            .unwrap()
            .run_with_deadline(Duration::ZERO)
            .unwrap();
        assert!(
            result.has_more(),
            "expired deadline must request continuation"
        );
        f.engine.finish_key_usage(Ok(result));
        let empty = f.view(&key).usage.unwrap();
        assert_eq!(empty.windows.total, CodexTokenCounts::default());
        let persisted = UsageStore::new(f.vault.directory().join("codex-key-usage.json")).load();
        assert_eq!(persisted.keys[&key].all_time.records, 0);
        scan(&mut f, now);
        let counted = f
            .engine
            .snapshot(false, now, &CodexPolicy::default())
            .accounts
            .into_iter()
            .find(|a| a.id == key)
            .unwrap()
            .usage
            .unwrap();
        let expected = CodexTokenCounts {
            input: 7,
            cached: 3,
            output: 4,
            total: 14,
        };
        assert_eq!(counted.windows.last_24h, expected);
        assert_eq!(counted.windows.total, expected);
        assert!(f.engine.plan_key_usage(now).is_none());
    }
}

#[tokio::test]
async fn only_proven_key_selections_open_extend_and_close_activation_spans() {
    let mut f = Fixture::new();
    f.add_key("First", FAKE_KEY).await.unwrap();
    f.add_key("Second", SECOND_KEY).await.unwrap();
    let first = f.key_id(FAKE_KEY);
    let second = f.key_id(SECOND_KEY);
    assert!(spans(&f, &first).is_empty());
    f.switch(&first).await.unwrap();
    assert_eq!(
        spans(&f, &first),
        vec![ActivationSpan {
            start: 200,
            end: 210
        }]
    );
    f.engine.observe(&f.vault, 300).unwrap();
    assert_eq!(
        spans(&f, &first),
        vec![ActivationSpan {
            start: 200,
            end: 300
        }]
    );
    let auth_path = f.home.join("auth.json");
    fs::remove_file(&auth_path).unwrap();
    fs::create_dir(&auth_path).unwrap();
    assert!(f.engine.observe(&f.vault, 350).is_err());
    assert_eq!(spans(&f, &first)[0].end, 300);
    fs::remove_dir(&auth_path).unwrap();
    // Discovering an external replacement proves nothing about the intervening gap.
    f.write_active(&auth("user-a", "ws-a", "external"));
    f.engine.observe(&f.vault, 400).unwrap();
    assert_eq!(spans(&f, &first)[0].end, 300);
    f.write_active(&key_file(SECOND_KEY));
    f.engine.observe(&f.vault, 500).unwrap();
    f.write_active(&key_file(FAKE_KEY));
    f.engine.observe(&f.vault, 600).unwrap();
    assert_eq!(
        spans(&f, &second),
        vec![ActivationSpan {
            start: 500,
            end: 500
        }]
    );
    assert_eq!(
        spans(&f, &first),
        vec![
            ActivationSpan {
                start: 200,
                end: 300
            },
            ActivationSpan {
                start: 600,
                end: 600
            }
        ]
    );
    assert_eq!(
        f.saved_state()
            .accounts
            .iter()
            .find(|a| a.id == first)
            .unwrap()
            .usage_spans,
        spans(&f, &first)
    );
    // A manual key-to-key replacement owns the boundary second only once.
    let job = f
        .engine
        .begin_switch(&second, &f.vault, 700)
        .unwrap()
        .unwrap();
    let outcome = job.run().await;
    f.engine
        .finish_switch(&job, &outcome, &f.vault, 710, true)
        .unwrap();
    assert_eq!(
        spans(&f, &first)[1],
        ActivationSpan {
            start: 600,
            end: 699
        }
    );
    assert_eq!(
        spans(&f, &second)[1],
        ActivationSpan {
            start: 700,
            end: 710
        }
    );
    f.claude_untouched();
}

#[tokio::test]
async fn failed_login_observation_does_not_bridge_an_unobserved_external_login() {
    assert_observation_gap(true).await;
}

#[tokio::test]
async fn failed_login_observation_does_not_bridge_even_an_unchanged_login() {
    assert_observation_gap(false).await;
}

async fn assert_observation_gap(external_login: bool) {
    let mut f = Fixture::new();
    f.add_key("Key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    f.switch(&key).await.unwrap();
    f.engine.observe(&f.vault, 300).unwrap();
    let auth_path = f.home.join("auth.json");
    fs::remove_file(&auth_path).unwrap();
    fs::create_dir(&auth_path).unwrap();
    assert!(f.engine.observe(&f.vault, 350).is_err());
    assert_eq!(
        spans(&f, &key),
        vec![ActivationSpan {
            start: 200,
            end: 300
        }]
    );
    fs::remove_dir(&auth_path).unwrap();
    if external_login {
        f.write_active(&auth("user-a", "ws-a", "unobserved"));
    }
    f.write_active(&key_file(FAKE_KEY));
    f.engine.observe(&f.vault, 600).unwrap();
    let expected = vec![
        ActivationSpan {
            start: 200,
            end: 300,
        },
        ActivationSpan {
            start: 600,
            end: 600,
        },
    ];
    assert_eq!(spans(&f, &key), expected);
    assert_eq!(
        f.saved_state()
            .accounts
            .iter()
            .find(|a| a.id == key)
            .unwrap()
            .usage_spans,
        expected
    );
    // The unobserved interval is never charged, including an unchanged key.
    rollout(
        &f,
        &(usage_record("before-gap", "1970-01-01T00:05:00Z")
            + &usage_record("during-gap", "1970-01-01T00:07:30Z")
            + &usage_record("after-gap", "1970-01-01T00:10:00Z")),
    );
    scan(&mut f, 600);
    assert_eq!(f.view(&key).usage.unwrap().windows.total.total, 28);
    f.claude_untouched();
}

#[tokio::test]
async fn unavailable_home_and_failed_import_interrupt_key_observation() {
    for unavailable_home in [false, true] {
        let mut f = Fixture::new();
        f.add_key("Key", FAKE_KEY).await.unwrap();
        let key = f.key_id(FAKE_KEY);
        f.switch(&key).await.unwrap();
        f.engine.observe(&f.vault, 300).unwrap();
        if unavailable_home {
            let installation = f.engine.installation.take();
            f.engine.observe(&f.vault, 350).unwrap();
            f.engine.installation = installation;
        } else {
            let path = f.home.join("auth.json");
            fs::remove_file(&path).unwrap();
            fs::create_dir(&path).unwrap();
            assert!(
                f.engine
                    .command(CodexCommand::ImportCurrent, &f.vault, 350)
                    .await
                    .is_err()
            );
            fs::remove_dir(&path).unwrap();
            f.write_active(&key_file(FAKE_KEY));
        }
        f.engine.observe(&f.vault, 600).unwrap();
        assert_eq!(
            spans(&f, &key),
            vec![
                ActivationSpan {
                    start: 200,
                    end: 300
                },
                ActivationSpan {
                    start: 600,
                    end: 600
                }
            ]
        );
    }
}

#[tokio::test]
async fn skipped_observation_during_switch_demo_or_read_only_keeps_existing_evidence() {
    for mode in ["switch", "demo", "read-only"] {
        let mut f = Fixture::new();
        f.add_key("Key", FAKE_KEY).await.unwrap();
        let key = f.key_id(FAKE_KEY);
        let job = f.engine.begin_switch(&key, &f.vault, 200).unwrap().unwrap();
        let outcome = job.run().await;
        if mode != "switch" {
            f.engine
                .finish_switch(&job, &outcome, &f.vault, 210, true)
                .unwrap();
        }
        f.engine.demo = mode == "demo";
        f.engine.writable = mode != "read-only";
        let path = f.home.join("auth.json");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        f.engine.observe(&f.vault, 350).unwrap();
        fs::remove_dir(&path).unwrap();
        f.write_active(&key_file(FAKE_KEY));
        f.engine.demo = false;
        f.engine.writable = true;
        if mode == "switch" {
            f.engine
                .finish_switch(&job, &outcome, &f.vault, 600, true)
                .unwrap();
        } else {
            f.engine.observe(&f.vault, 600).unwrap();
        }
        assert_eq!(
            spans(&f, &key),
            vec![ActivationSpan {
                start: 200,
                end: 600
            }]
        );
    }
}

fn saved_span(f: &Fixture, key: &str) -> ActivationSpan {
    *f.saved_state()
        .accounts
        .iter()
        .find(|a| a.id == key)
        .unwrap()
        .usage_spans
        .last()
        .unwrap()
}

#[tokio::test]
async fn ordinary_ticks_keep_spans_current_without_vault_writes_until_saved_end_lags_300_seconds() {
    let mut f = Fixture::new();
    f.add_key("Key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    f.switch(&key).await.unwrap();
    let path = f.vault.directory().join("codex-state.vault");
    let ciphertext = fs::read(&path).unwrap();
    for at in [211, 270, 330, 390, 450, 509] {
        f.engine.observe(&f.vault, at).unwrap();
        assert_eq!(spans(&f, &key)[0].end, at);
        assert_eq!(saved_span(&f, &key).end, 210);
        assert_eq!(
            fs::read(&path).unwrap(),
            ciphertext,
            "ordinary tick rewrote vault"
        );
        scan(&mut f, at); // Every extension must still schedule a scan.
    }
    f.engine.observe(&f.vault, 510).unwrap();
    assert_eq!(saved_span(&f, &key).end, 510);
    assert_ne!(fs::read(&path).unwrap(), ciphertext);
    // Any other successful account save also checkpoints the end actually written.
    f.engine.observe(&f.vault, 550).unwrap();
    f.engine.save(&f.vault).unwrap();
    let ciphertext = fs::read(&path).unwrap();
    f.engine.observe(&f.vault, 849).unwrap();
    assert_eq!(saved_span(&f, &key).end, 550);
    assert_eq!(fs::read(&path).unwrap(), ciphertext);
    f.engine.observe(&f.vault, 850).unwrap();
    assert_eq!(saved_span(&f, &key).end, 850);
    f.engine.observe(&f.vault, 900).unwrap();
    f.write_active(&auth("user-a", "ws-a", "external"));
    f.engine.observe(&f.vault, 950).unwrap();
    assert_eq!(
        saved_span(&f, &key).end,
        900,
        "closing saves the last proven end"
    );
}

#[tokio::test]
async fn restart_uses_the_saved_end_even_when_the_in_memory_end_was_newer() {
    for gap in [600, 601] {
        let mut f = Fixture::new();
        f.add_key("Key", FAKE_KEY).await.unwrap();
        let key = f.key_id(FAKE_KEY);
        f.switch(&key).await.unwrap();
        f.engine.observe(&f.vault, 500).unwrap();
        assert_eq!(saved_span(&f, &key).end, 210);
        reopen(&mut f, 210 + gap);
        if gap == 600 {
            assert_eq!(
                spans(&f, &key),
                vec![ActivationSpan {
                    start: 200,
                    end: 810
                }]
            );
        } else {
            assert_eq!(
                spans(&f, &key),
                vec![
                    ActivationSpan {
                        start: 200,
                        end: 210
                    },
                    ActivationSpan {
                        start: 811,
                        end: 811
                    }
                ]
            );
        }
    }
}

#[tokio::test]
async fn idle_scans_preserve_the_ledger_file_generation_including_after_restart() {
    let mut f = Fixture::new();
    f.add_key("Key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    f.switch(&key).await.unwrap();
    rollout(&f, &usage_record("first", "1970-01-01T00:03:25Z"));
    scan(&mut f, 210);
    let path = f.vault.directory().join("codex-key-usage.json");
    let (receipt, _) = CodexWatchedSource::capture(CodexSourceRole::User, path.clone()).unwrap();
    for now in [211, 212] {
        f.engine.observe(&f.vault, now).unwrap();
        scan(&mut f, now);
        receipt
            .revalidate()
            .expect("idle scan must not replace the ledger");
    }
    reopen(&mut f, 213);
    scan(&mut f, 213);
    receipt
        .revalidate()
        .expect("unchanged catch-up must not rewrite the ledger");
    assert_eq!(f.view(&key).usage.unwrap().windows.total.total, 14);
    // Native generation catches replacement even when serialized bytes match.
    let store = UsageStore::new(path);
    store.save(&store.load()).unwrap();
    assert!(receipt.revalidate().is_err());
}

#[tokio::test]
async fn a_restart_resumes_at_ten_minutes_but_never_bridges_a_longer_gap() {
    for gap in [600, 601] {
        let mut f = Fixture::new();
        f.add_key("Key", FAKE_KEY).await.unwrap();
        let key = f.key_id(FAKE_KEY);
        f.switch(&key).await.unwrap();
        reopen(&mut f, 210 + gap);
        let actual = spans(&f, &key);
        if gap == 600 {
            assert_eq!(
                actual,
                vec![ActivationSpan {
                    start: 200,
                    end: 810
                }]
            );
        } else {
            assert_eq!(
                actual,
                vec![
                    ActivationSpan {
                        start: 200,
                        end: 210
                    },
                    ActivationSpan {
                        start: 811,
                        end: 811
                    }
                ]
            );
        }
    }
}

#[tokio::test]
async fn span_history_retains_the_newest_two_hundred_proven_activations() {
    let mut f = Fixture::new();
    f.add_key("Key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    let index = f.engine.account_index(&key).unwrap();
    f.engine.saved.accounts[index].usage_spans = (0..200)
        .map(|at| ActivationSpan {
            start: at * 2,
            end: at * 2,
        })
        .collect();
    f.write_active(&key_file(FAKE_KEY));
    f.engine.observe(&f.vault, 1000).unwrap();
    let actual = spans(&f, &key);
    assert_eq!(actual.len(), 200);
    assert_eq!(actual[0], ActivationSpan { start: 2, end: 2 });
    assert_eq!(
        actual[199],
        ActivationSpan {
            start: 1000,
            end: 1000
        }
    );
    reopen(&mut f, 1001);
    assert_eq!(spans(&f, &key).len(), 200);
    assert_eq!(spans(&f, &key)[199].end, 1001);
}

#[tokio::test]
async fn failed_flex_or_login_writes_do_not_claim_that_the_key_was_active() {
    for malformed_key in [false, true] {
        let mut f = Fixture::new();
        f.flex_ready(if malformed_key {
            OWNER_CONFIG
        } else {
            b"service_tier = 5\n"
        });
        let key = f.flex_key("Key", FAKE_KEY).await;
        if malformed_key {
            let index = f.engine.account_index(&key).unwrap();
            f.engine.saved.accounts[index].auth =
                br#"{"auth_mode":"apikey","OPENAI_API_KEY":" "}"#.to_vec();
        }
        let before = f.active();
        assert_eq!(
            f.switch(&key).await.err(),
            Some(if malformed_key {
                CodexReason::UnsupportedAuth
            } else {
                CodexReason::FlexConfigFailed
            })
        );
        assert_eq!(f.active(), before);
        assert!(spans(&f, &key).is_empty());
        assert!(
            f.saved_state()
                .accounts
                .iter()
                .find(|a| a.id == key)
                .unwrap()
                .usage_spans
                .is_empty()
        );
        assert!(f.daemon.calls.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn a_failed_import_rolls_back_any_new_span_and_selection() {
    let mut f = Fixture::new();
    let chatgpt = f.id_of("ws-a");
    f.add_key("Key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    f.write_active(&key_file(FAKE_KEY));
    f.break_vault();
    assert_eq!(
        f.engine
            .command(CodexCommand::ImportCurrent, &f.vault, 400)
            .await
            .err(),
        Some(CodexReason::VaultUnavailable)
    );
    assert_eq!(f.engine.selected_id.as_deref(), Some(chatgpt.as_str()));
    assert!(spans(&f, &key).is_empty());
    assert_eq!(f.active(), key_file(FAKE_KEY));
    assert!(f.engine.plan_key_usage(400).is_none());
}

#[tokio::test]
async fn importing_a_live_key_selects_and_records_it_including_late_chatgpt_refreshes() {
    for hybrid in [false, true] {
        let mut f = Fixture::new();
        f.add_key("Key", FAKE_KEY).await.unwrap();
        let key = f.key_id(FAKE_KEY);
        f.write_active(&if hybrid {
            key_with_tokens(FAKE_KEY, "user-a", "ws-a", "late-refresh")
        } else {
            key_file(FAKE_KEY)
        });
        f.engine
            .command(CodexCommand::ImportCurrent, &f.vault, 400)
            .await
            .unwrap();
        assert_eq!(f.engine.selected_id.as_deref(), Some(key.as_str()));
        assert_eq!(
            spans(&f, &key),
            vec![ActivationSpan {
                start: 400,
                end: 400
            }]
        );
        assert_eq!(f.engine.saved.accounts.len(), 2);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn switching_away_from_a_key_schedules_a_final_scan_before_daemon_completion() {
    let mut f = Fixture::new();
    let chatgpt = f.id_of("ws-a");
    f.add_key("Key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    f.switch(&key).await.unwrap();
    scan(&mut f, 210);
    assert!(f.engine.plan_key_usage(210).is_none());
    // Flushed after the prior scan: only the queued final scan can count it.
    rollout(&f, &usage_record("outgoing", "1970-01-01T00:03:25Z"));
    f.factory
        .enqueue(Plan::quota(Ok(quota(Some("ws-a"), 0, 0))));
    let calls_before = f.daemon.calls.lock().unwrap().len();
    let daemon = f.daemon.clone();
    let store = UsageStore::new(f.vault.directory().join("codex-key-usage.json"));
    let (_temp, handle) = runtime(&mut f, Settings::default()).await;
    let (started_tx, mut started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let started_tx = Mutex::new(Some(started_tx));
    let release_rx = Mutex::new(release_rx);
    handle
        .edit_codex_for_test(move |engine, _| {
            engine.usage_scan_hook(Arc::new(move || {
                let restarted = daemon.calls.lock().unwrap()[calls_before..].contains(&"restart");
                if let Some(sender) = started_tx.lock().unwrap().take() {
                    let _ = sender.send(restarted);
                }
                let _ = release_rx.lock().unwrap().recv();
            }));
        })
        .await;
    let switching = handle.clone();
    let mut switch = tokio::spawn(async move { switching.codex_switch_account(&chatgpt).await });
    let (finished_before_scan, restarted, early_result) = tokio::select! {
        result = &mut switch => (true, started_rx.await.unwrap(), Some(result)),
        restarted = &mut started_rx => (false, restarted.unwrap(), None),
    };
    let released = release_tx.send(());
    let result = match early_result {
        Some(result) => result,
        None => switch.await,
    };
    handle.wait_codex_jobs_for_test().await;
    assert!(released.is_ok());
    assert!(
        finished_before_scan,
        "daemon completion waited for the usage worker"
    );
    assert!(
        restarted,
        "scan ran between login replacement and daemon restart"
    );
    assert!(result.unwrap().is_ok());
    assert_eq!(store.load().keys[&key].all_time.total, 14);
    handle
        .edit_codex_for_test(move |engine, _| {
            let account = &engine.saved.accounts[engine.account_index(&key).unwrap()];
            assert_eq!(
                account.usage_spans,
                vec![ActivationSpan {
                    start: 200,
                    end: NOW - 1
                }]
            );
        })
        .await;
    handle.shutdown_codex().await;
    f.claude_untouched();
}

#[tokio::test]
async fn a_failed_final_login_read_cannot_extend_the_successful_key_write_evidence() {
    let mut f = Fixture::new();
    f.add_key("Key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    let job = f.engine.begin_switch(&key, &f.vault, 200).unwrap().unwrap();
    assert_eq!(
        spans(&f, &key),
        vec![ActivationSpan {
            start: 200,
            end: 200
        }]
    );
    let outcome = job.run().await;
    // Complete the scan supported by the successful write before the failed read.
    scan(&mut f, 200);
    assert!(f.engine.plan_key_usage(200).is_none());
    let auth_path = f.home.join("auth.json");
    fs::remove_file(&auth_path).unwrap();
    fs::create_dir(&auth_path).unwrap();
    let _ = f.engine.finish_switch(&job, &outcome, &f.vault, 210, true);
    assert_eq!(
        spans(&f, &key),
        vec![ActivationSpan {
            start: 200,
            end: 200
        }]
    );
    assert!(f.engine.plan_key_usage(210).is_none());
    f.claude_untouched();
}

fn usage_record(id: &str, timestamp: &str) -> String {
    json!({
        "type":"token_usage_record", "timestamp":timestamp,
        "payload":{
            "response_id":id,
            "usage":{"input_tokens":10,"cached_input_tokens":3,"output_tokens":4,
                "reasoning_output_tokens":999,"total_tokens":14},
            "turn_token_usage":{"input_tokens":900000,"total_tokens":900000},
            "thread_token_usage":{"input_tokens":9000000,"total_tokens":9000000},
            "text":"PRIVATE_MESSAGE_SENTINEL", "key":FAKE_KEY
        }
    })
    .to_string()
        + "\n"
}

fn rollout(f: &Fixture, data: &str) {
    let directory = f.home.join("sessions/1970/02/09");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("rollout-fixture.jsonl"), data).unwrap();
}

#[tokio::test]
async fn synthetic_rollouts_produce_exact_key_windows_without_quota_or_secret_disclosure() {
    const DAY: i64 = 86_400;
    let mut f = Fixture::new();
    let chatgpt = f.id_of("ws-a");
    f.add_key("Key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    f.write_active(&key_file(FAKE_KEY));
    f.engine.observe(&f.vault, DAY).unwrap();
    f.engine.observe(&f.vault, 40 * DAY).unwrap();
    rollout(
        &f,
        &[
            usage_record("PRIVATE_RESPONSE_SENTINEL-old", "1970-01-02T00:00:00Z"),
            usage_record("PRIVATE_RESPONSE_SENTINEL-month", "1970-01-15T00:00:00Z"),
            usage_record("PRIVATE_RESPONSE_SENTINEL-week", "1970-02-05T00:00:00Z"),
            usage_record("PRIVATE_RESPONSE_SENTINEL-day", "1970-02-09T12:00:00Z"),
            usage_record("PRIVATE_RESPONSE_SENTINEL-day", "1970-02-09T12:00:00Z"),
            usage_record(
                "PRIVATE_RESPONSE_SENTINEL-unattributed",
                "1970-01-01T00:01:40Z",
            ),
            json!({"type":"event_msg","timestamp":"1970-02-09T12:00:00Z",
            "payload":{"type":"token_count","total_token_usage":{"total_tokens":99999999}}})
            .to_string()
                + "\n",
        ]
        .concat(),
    );
    scan(&mut f, 40 * DAY);
    let snapshot = f.engine.snapshot(false, 40 * DAY, &CodexPolicy::default());
    let view = snapshot.accounts.iter().find(|a| a.id == key).unwrap();
    assert!(view.quota.is_none());
    let usage = view.usage.as_ref().unwrap();
    assert_eq!(usage.tracked_since, Some(DAY));
    assert_eq!(usage.state, CodexUsageState::Idle);
    for (window, records) in [
        (&usage.windows.last_24h, 1),
        (&usage.windows.last_7d, 2),
        (&usage.windows.last_30d, 3),
        (&usage.windows.total, 4),
    ] {
        assert_eq!(
            *window,
            CodexTokenCounts {
                input: records * 7,
                cached: records * 3,
                output: records * 4,
                total: records * 14
            }
        );
    }
    assert!(
        snapshot
            .accounts
            .iter()
            .find(|a| a.id == chatgpt)
            .unwrap()
            .usage
            .is_none()
    );
    let public = serde_json::to_string(&snapshot).unwrap();
    let row = serde_json::to_value(view).unwrap();
    assert_eq!(row["usage"]["windows"]["last24h"]["input"], 7);
    assert_eq!(row["usage"]["windows"]["last7d"]["cached"], 6);
    assert_eq!(row["usage"]["windows"]["last30d"]["output"], 12);
    let ledger_path = f.vault.directory().join("codex-key-usage.json");
    let ledger = fs::read_to_string(&ledger_path).unwrap();
    for secret in [
        FAKE_KEY,
        "PRIVATE_MESSAGE_SENTINEL",
        "PRIVATE_RESPONSE_SENTINEL",
        "thread_token_usage",
    ] {
        assert!(!public.contains(secret));
        assert!(!ledger.contains(secret));
    }
    // An optional ledger write failure exposes only a fixed reason and keeps switching usable.
    fs::remove_file(&ledger_path).unwrap();
    fs::create_dir(&ledger_path).unwrap();
    // A changed offset/record still requires persistence; unchanged scans now
    // deliberately avoid writing even when the optional store is unavailable.
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(f.home.join("sessions/1970/02/09/rollout-fixture.jsonl"))
        .unwrap()
        .write_all(usage_record("new-record", "1970-02-10T00:00:00Z").as_bytes())
        .unwrap();
    f.engine.observe(&f.vault, 40 * DAY + 1).unwrap();
    let error = f
        .engine
        .plan_key_usage(40 * DAY + 1)
        .unwrap()
        .run()
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "usageSaveFailed");
    f.engine.finish_key_usage(Err(error));
    assert_eq!(
        f.view(&key).usage.unwrap().state,
        CodexUsageState::Unavailable
    );
    f.claude_untouched();
}

#[tokio::test]
async fn old_accounts_omit_empty_spans_and_a_corrupt_optional_ledger_is_ignored() {
    let mut f = Fixture::new();
    f.add_key("Key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    let old = serde_json::to_value(&f.engine.saved).unwrap();
    assert!(
        old["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a.get("usage_spans").is_none())
    );
    let loaded: Saved = serde_json::from_value(old.clone()).unwrap();
    assert!(loaded.accounts.iter().all(|a| a.usage_spans.is_empty()));
    assert_eq!(serde_json::to_value(loaded).unwrap(), old);
    fs::write(
        f.vault.directory().join("codex-key-usage.json"),
        b"{corrupt-PRIVATE_MESSAGE_SENTINEL",
    )
    .unwrap();
    reopen(&mut f, 400);
    scan(&mut f, 400);
    let view = f.view(&key).usage.unwrap();
    assert_eq!(view.tracked_since, None);
    assert_eq!(view.windows, CodexUsageWindows::default());
    assert_eq!(view.state, CodexUsageState::Idle);
    let persisted = fs::read_to_string(f.vault.directory().join("codex-key-usage.json")).unwrap();
    assert!(!persisted.contains("PRIVATE_MESSAGE_SENTINEL"));
    assert_eq!(f.active(), auth("user-a", "ws-a", "initial"));
}

#[tokio::test]
async fn deletion_through_the_runtime_handle_removes_consumption_from_the_persisted_ledger() {
    let mut f = Fixture::new();
    f.add_key("Key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    f.switch(&key).await.unwrap();
    rollout(
        &f,
        &usage_record("PRIVATE_RESPONSE_SENTINEL", "1970-01-01T00:03:25Z"),
    );
    scan(&mut f, 210);
    let store = UsageStore::new(f.vault.directory().join("codex-key-usage.json"));
    assert_eq!(store.load().keys[&key].all_time.total, 14);
    f.write_active(&auth("user-a", "ws-a", "external"));
    f.engine.observe(&f.vault, 300).unwrap();
    scan(&mut f, 300);
    let (_temp, handle) = runtime(&mut f, Settings::default()).await;
    let snapshot = handle.codex_delete_account(&key).await.unwrap();
    assert!(snapshot.accounts.iter().all(|a| a.id != key));
    assert!(!store.load().keys.contains_key(&key));
    // A later ordinary saved-account mutation cannot resurrect the deleted counters.
    let chatgpt = snapshot.accounts[0].id.clone();
    handle
        .codex_update_account_automation(
            &chatgpt,
            AccountAutomationView {
                enabled: true,
                threshold: None,
                priority: Some(1),
                return_on_five_hour_reset: false,
            },
        )
        .await
        .unwrap();
    assert!(!store.load().keys.contains_key(&key));
    handle.shutdown_codex().await;
}

#[tokio::test]
async fn automatic_fallback_and_return_use_the_same_activation_evidence_hooks() {
    let (mut f, _a, b, key) = exhausted_world(false).await;
    let policy = CodexPolicy::default();
    let plan = plan_of(f.engine.automation(NOW, &policy));
    assert_eq!(plan.kind, CodexAutoKind::KeyFallback);
    f.auto_switch(&plan, NOW).await.unwrap();
    assert_eq!(
        spans(&f, &key),
        vec![ActivationSpan {
            start: NOW,
            end: NOW + 1
        }]
    );
    let later = NOW + AUTO_SWITCH_COOLDOWN;
    f.read(&b, main_quota(10, 10), later);
    let plan = plan_of(f.engine.automation(later, &policy));
    assert_eq!(plan.kind, CodexAutoKind::KeyReturn);
    f.auto_switch(&plan, later).await.unwrap();
    assert_eq!(
        spans(&f, &key),
        vec![ActivationSpan {
            start: NOW,
            end: later - 1
        }]
    );
    assert_eq!(f.engine.selected_id.as_deref(), Some(b.as_str()));
    assert!(f.engine.plan_key_usage(later + 1).is_some());
    f.claude_untouched();
}

#[tokio::test(flavor = "current_thread")]
async fn blocking_rollout_work_leaves_the_runtime_owner_responsive() {
    let mut f = Fixture::new();
    let (_temp, handle) = runtime(&mut f, Settings::default()).await;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let started_tx = Mutex::new(Some(started_tx));
    let release_rx = Mutex::new(release_rx);
    let runtime_thread = std::thread::current().id();
    let hook = Arc::new(move || {
        let off_runtime_thread = std::thread::current().id() != runtime_thread;
        if let Some(sender) = started_tx.lock().unwrap().take() {
            let _ = sender.send(off_runtime_thread);
        }
        if off_runtime_thread {
            // The controller releases this handshake before any assertion.
            // If spawn_blocking regresses, never block the test's only executor.
            let _ = release_rx.lock().unwrap().recv();
        }
    });
    handle
        .edit_codex_for_test(move |engine, _| engine.usage_scan_hook(hook))
        .await;
    let scanner = handle.clone();
    let mut task = tokio::spawn(async move { scanner.codex_scan_key_usage().await });
    let off_runtime_thread = tokio::select! {
        started = started_rx => started.unwrap(),
        finished = &mut task => panic!("scan finished before the worker handshake: {finished:?}"),
    };
    let responsive = handle.try_edit_codex_for_test(|_, settings| settings.language = "ro".into());
    // Unblock before any assertions, including on a failed responsiveness check.
    let released = release_tx.send(());
    task.await.unwrap();
    assert!(off_runtime_thread, "rollout work ran on the async executor");
    assert!(
        responsive,
        "runtime owner was held during blocking usage work"
    );
    assert!(
        released.is_ok(),
        "blocking job was not paused while the owner responded"
    );
    handle.shutdown_codex().await;
}

#[tokio::test]
async fn optional_ledger_values_are_clamped_to_safe_ipc_integers_and_displayable_dates() {
    const JS_MAX: u64 = (1 << 53) - 1;
    let mut f = Fixture::new();
    f.add_key("Key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    let store = UsageStore::new(f.vault.directory().join("codex-key-usage.json"));
    for (since, expected_date) in [(i64::MAX, 253_402_300_799), (-100, 0)] {
        let counts = UsageCounters {
            input_uncached: u64::MAX,
            cached: u64::MAX,
            output: u64::MAX,
            total: u64::MAX,
            records: 1,
        };
        let mut ledger = UsageLedger::default();
        ledger.keys.insert(
            key.clone(),
            KeyUsageLedger {
                tracking_since: since,
                all_time: counts,
                buckets: BTreeMap::from([(0, counts)]),
            },
        );
        store.save(&ledger).unwrap();
        reopen(&mut f, 400);
        scan(&mut f, 400);
        let usage = f.view(&key).usage.unwrap();
        assert_eq!(usage.tracked_since, Some(expected_date));
        for window in [
            &usage.windows.last_24h,
            &usage.windows.last_7d,
            &usage.windows.last_30d,
            &usage.windows.total,
        ] {
            assert_eq!(
                *window,
                CodexTokenCounts {
                    input: JS_MAX,
                    cached: JS_MAX,
                    output: JS_MAX,
                    total: JS_MAX,
                }
            );
        }
    }
}
