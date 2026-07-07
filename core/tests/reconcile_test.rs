//! Testes da lógica pura de diff da reconciliação (`reconcile::diff`) — §7.
//! Cobre: bancos iguais, faltando de cada lado, versão mais nova de cada lado,
//! e soft-delete (tratado como "mais novo" pelo `updated_at`).

use chrono::NaiveDate;
use uuid::Uuid;

use letaf_core::reconcile::{diff, ManifestEntry};

fn ts(day: u32) -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 1, day).unwrap().and_hms_opt(12, 0, 0).unwrap()
}
fn entry(id: Uuid, day: u32) -> ManifestEntry {
    ManifestEntry { id, updated_at: ts(day), deleted_at: None }
}

#[test]
fn identical_banks_have_no_drift() {
    let a = Uuid::new_v4();
    let local = vec![entry(a, 5)];
    let server = vec![entry(a, 5)];
    let d = diff(&local, &server);
    assert!(!d.server_drift);
    assert!(d.push_ids.is_empty());
}

#[test]
fn record_missing_locally_triggers_server_drift() {
    let a = Uuid::new_v4();
    let d = diff(&[], &[entry(a, 5)]);
    assert!(d.server_drift, "registro só no servidor → re-pull");
    assert!(d.push_ids.is_empty());
}

#[test]
fn record_missing_on_server_is_pushed() {
    let a = Uuid::new_v4();
    let d = diff(&[entry(a, 5)], &[]);
    assert!(!d.server_drift);
    assert_eq!(d.push_ids, vec![a], "registro só no local → re-push");
}

#[test]
fn server_newer_triggers_drift_only() {
    let a = Uuid::new_v4();
    let d = diff(&[entry(a, 5)], &[entry(a, 9)]); // servidor mais novo
    assert!(d.server_drift);
    assert!(d.push_ids.is_empty());
}

#[test]
fn local_newer_is_pushed_only() {
    let a = Uuid::new_v4();
    let d = diff(&[entry(a, 9)], &[entry(a, 5)]); // local mais novo
    assert!(!d.server_drift);
    assert_eq!(d.push_ids, vec![a]);
}

#[test]
fn soft_delete_newer_on_server_triggers_drift() {
    // Servidor tem o registro soft-deletado (updated_at mais novo) → o local
    // (ativo, mais antigo) deve re-puxar e aplicar a exclusão via LWW.
    let a = Uuid::new_v4();
    let local = vec![entry(a, 5)];
    let server = vec![ManifestEntry { id: a, updated_at: ts(9), deleted_at: Some(ts(9)) }];
    let d = diff(&local, &server);
    assert!(d.server_drift);
    assert!(d.push_ids.is_empty());
}

#[test]
fn divergence_in_both_directions() {
    let only_local = Uuid::new_v4();
    let only_server = Uuid::new_v4();
    let local = vec![entry(only_local, 5)];
    let server = vec![entry(only_server, 5)];
    let d = diff(&local, &server);
    assert!(d.server_drift);
    assert_eq!(d.push_ids, vec![only_local]);
}
