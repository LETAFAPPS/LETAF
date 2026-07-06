use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Impressora cadastrada localmente no desktop.
///
/// Regras aplicadas (AI_RULES.md §6, §8, §11):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced).
/// - `system_name` é o **nome da impressora no SO** (ex.: "EPSON TM-T20",
///   "ZJ-58 Printer"). Será usado em runtime como argumento de `lp -d`
///   no Linux/macOS ou `Out-Printer -Name` no Windows.
/// - `kind` define para que tipo de documento a impressora serve: comanda
///   do cliente, ticket de cozinha ou NFC-e. A regra "1 padrão por tipo"
///   é forçada pelo service (em transação).
/// - **Não sincroniza com servidor**: impressora é per-device. O campo
///   `synced` da BaseFields fica permanentemente `true` para impedir
///   tentativas de push pelo SyncWorker (§7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Printer {
    #[serde(flatten)]
    pub base: BaseFields,
    /// Rótulo livre que o operador vê na lista — ex.: "Cozinha 1",
    /// "Balcão". Não precisa ser único.
    pub name: String,
    /// `"order"` (comanda do cliente) | `"kitchen"` (ticket cozinha)
    /// | `"fiscal"` (NFC-e — esqueleto, fluxo NFC-e fica para Fase 2).
    pub kind: String,
    /// Nome exato como aparece no SO. Distros Linux: `lpstat -p`.
    /// Windows: "Painel de Controle > Dispositivos e Impressoras".
    pub system_name: String,
    /// Impressora padrão para o `kind`. Apenas 1 por tipo (forçado
    /// pelo service via UPDATE em transação).
    pub is_default: bool,
    /// Largura do papel em mm — 58 ou 80 (térmicas mais comuns).
    /// Afeta a quantidade de colunas do texto monospace gerado.
    pub paper_width: i32,
    /// IDs de categorias que esta impressora atende. Quando vazio, a
    /// impressora age como "catch-all" — recebe todos os itens da
    /// cozinha. Quando há ao menos um ID, só recebe itens de produtos
    /// dessas categorias (roteamento por estação).
    ///
    /// Atualmente afeta apenas comandas com `kind == "kitchen"`. A
    /// comanda do cliente (`kind == "order"`) ignora esse campo —
    /// sempre imprime tudo num único papel.
    #[serde(default)]
    pub category_ids: Vec<Uuid>,
}

impl Printer {
    pub fn new(
        company_id: Uuid,
        name: String,
        kind: String,
        system_name: String,
        is_default: bool,
        paper_width: i32,
        category_ids: Vec<Uuid>,
    ) -> Self {
        // Impressora é per-device → marca como já sincronizado para o
        // SyncWorker nunca tentar enviar.
        let mut base = BaseFields::new(company_id);
        base.synced = true;
        Self {
            base,
            name,
            kind,
            system_name,
            is_default,
            paper_width,
            category_ids,
        }
    }
}

/// Tipos válidos para `Printer.kind`. Mudanças aqui exigem migração.
pub const PRINTER_KINDS: &[&str] = &["order", "kitchen", "fiscal"];

/// Larguras de papel suportadas (mm). Térmicas residenciais raramente
/// fogem dessa lista; impressoras de mesa A4 podem ser cadastradas
/// com `80` (não usamos width pra A4 — o driver lida).
pub const PAPER_WIDTHS: &[i32] = &[58, 80];
