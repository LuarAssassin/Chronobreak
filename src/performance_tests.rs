use super::*;
use crate::{Semantics, message_channel};
use std::{num::NonZeroUsize, time::Instant};

fn distribution(name: &str, cold_us: u128, samples: &mut [u128], stats: &str) {
    samples.sort_unstable();
    let percentile = |percent: usize| samples[(samples.len() - 1) * percent / 100];
    eprintln!(
        "performance {}",
        serde_json::json!({
            "workload": name, "cold_us": cold_us, "samples": samples.len(),
            "p50_us": percentile(50), "p95_us": percentile(95), "p99_us": percentile(99),
            "max_us": samples.last().unwrap(), "final_resources": stats,
            "scope": "CPU retained scene, text shaping, layout, semantics; not GPU/present latency"
        })
    );
}

#[test]
#[ignore = "explicit serial hardware qualification; see scripts/qualify_performance.py"]
fn measured_alpha_workloads() {
    for workload in [
        "workbench128",
        "controls128",
        "unicode64",
        "dynamic128",
        "background64",
    ] {
        let value = Signal::new(0_u32);
        let expanded = Signal::new(true);
        let (dynamic, children) = Element::keyed_column();
        let (sender, receiver) = message_channel(NonZeroUsize::new(64).unwrap());
        let model = value.clone();
        let cold = Instant::now();
        let mut root = Element::column()
            .width(740.0)
            .child(dynamic)
            .on_message(receiver, move |message| {
                model.update(|value| *value = message)
            });
        let count = if workload == "unicode64" { 64 } else { 128 };
        if workload != "dynamic128" {
            for index in 0..count {
                let element = match workload {
                    "controls128" => Element::tree_header(format!("Control {index}"), &expanded)
                        .child(Element::checkbox("checked", &expanded)),
                    "unicode64" => Element::bound_text(&value, move |value| {
                        format!("{index}: 中文 e\u{301} 🌱 العربية — {value}")
                    }),
                    _ => Element::bound_text(&value, move |value| {
                        format!("Node {index}: source citation {value}")
                    }),
                };
                root = root.child(element.height(32.0));
            }
        }
        let mut scene = Scene::new(root);
        let semantics = Semantics::default();
        scene.set_semantics(semantics);
        scene.flush(760.0, 520.0);
        let cold_us = cold.elapsed().as_micros();
        let initial_nodes = scene.nodes.len();
        let initial_taffy = scene.tree.total_node_count();
        let mut samples = Vec::new();
        let mut warm_stats = None;
        let mut rss_samples = Vec::new();
        for cycle in 0..220 {
            let started = Instant::now();
            if workload == "dynamic128" {
                for index in 0..128 {
                    children.insert(
                        index.to_string(),
                        Element::text(format!("Dynamic {cycle}/{index}")),
                    );
                }
                scene.flush(760.0, 520.0);
                for index in 0..128 {
                    children.remove(children.node(&index.to_string()).unwrap());
                }
            } else if workload == "background64" {
                for message in 0..64 {
                    sender.try_send(cycle * 64 + message).unwrap();
                }
                assert_eq!(scene.drain_messages(), 64);
            } else if workload == "controls128" {
                expanded.update(|value| *value = !*value);
            } else {
                value.update(|value| *value = cycle);
            }
            scene.flush(760.0, 520.0);
            let _ = scene.display_list();
            scene.presented();
            let elapsed = started.elapsed().as_micros();
            assert_eq!(scene.nodes.len(), initial_nodes);
            assert_eq!(scene.tree.total_node_count(), initial_taffy);
            assert!(scene.pending.borrow().is_empty());
            assert!(scene.commands.borrow().is_empty());
            if [19, 69, 119, 169, 219].contains(&cycle) {
                let status = std::fs::read_to_string("/proc/self/status").unwrap();
                let rss = status
                    .lines()
                    .find(|line| line.starts_with("VmRSS:"))
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .parse::<u64>()
                    .unwrap();
                rss_samples.push((cycle, rss));
            }
            if cycle == 19 {
                warm_stats = Some(scene.stats());
            }
            if cycle >= 20 {
                assert_eq!(warm_stats.as_ref().unwrap(), &scene.stats());
                samples.push(elapsed);
            }
        }
        distribution(workload, cold_us, &mut samples, &scene.stats());
        eprintln!(
            "rss {}",
            serde_json::json!({"workload": workload, "samples_cycle_kib": rss_samples, "scope": "process RSS incl allocator/font caches; not live resources or VRAM"})
        );
        drop(scene);
        assert_eq!(value.subscriber_count(), 0);
        assert_eq!(expanded.subscriber_count(), 0);
    }
}
