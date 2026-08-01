//! Testes da lógica pura de diff da reconciliação (`reconcile::diff`) — §7.
//! Cobre: bancos iguais, faltando de cada lado, versão mais nova de cada lado,
//! e soft-delete (tratado como "mais novo" pelo `updated_at`).

use chrono::{NaiveDate, NaiveDateTime};
use uuid::Uuid;

use letaf_core::error::CoreError;
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

/// A paginação do manifesto NÃO pode inferir o fim comparando o tamanho da
/// página com uma constante: o servidor recorta o `limit` em silêncio. Um
/// cliente que pede mais do que o teto do servidor tomaria a 1ª página como o
/// manifesto inteiro e trataria o resto da tabela como "falta no servidor",
/// reenviando tudo a cada reconcile.
#[tokio::test]
async fn manifesto_pagina_ate_a_pagina_vazia_mesmo_com_teto_menor_no_servidor() {
    use letaf_core::reconcile::{full_manifest, ManifestEntry, ReconcileRepository};
    use std::sync::Mutex;

    /// Repositório que IGNORA o `limit` pedido e devolve no máximo 3 por
    /// página — simula o servidor com teto menor que o do cliente.
    struct RepoComTetoMenor {
        ids: Vec<Uuid>,
        paginas: Mutex<usize>,
    }

    #[async_trait::async_trait]
    impl ReconcileRepository for RepoComTetoMenor {
        async fn manifest_page(
            &self,
            _company_id: Uuid,
            _table: &str,
            after_id: Option<Uuid>,
            _limit: i64,
        ) -> Result<Vec<ManifestEntry>, CoreError> {
            *self.paginas.lock().unwrap() += 1;
            let inicio = match after_id {
                None => 0,
                Some(id) => self.ids.iter().position(|x| *x == id).unwrap() + 1,
            };
            Ok(self.ids[inicio..]
                .iter()
                .take(3)
                .map(|id| ManifestEntry {
                    id: *id,
                    updated_at: NaiveDateTime::default(),
                    deleted_at: None,
                })
                .collect())
        }
        async fn mark_unsynced(
            &self,
            _company_id: Uuid,
            _table: &str,
            _ids: &[Uuid],
        ) -> Result<(), CoreError> {
            Ok(())
        }
    }

    let mut ids: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();
    ids.sort();
    let repo = RepoComTetoMenor { ids: ids.clone(), paginas: Mutex::new(0) };

    let m = full_manifest(&repo, Uuid::new_v4(), "products").await.unwrap();
    let vistos: Vec<Uuid> = m.into_iter().map(|e| e.id).collect();

    assert_eq!(
        vistos, ids,
        "o manifesto tem que vir COMPLETO mesmo com o servidor recortando a página"
    );
    // 10 registros em páginas de 3 = 4 páginas com dado + 1 vazia.
    assert_eq!(*repo.paginas.lock().unwrap(), 5);
}
