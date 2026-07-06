use std::sync::Arc;

use chrono::{Datelike, Local, NaiveDate};
use serde::Deserialize;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::sync::{Notify, RwLock};

use letaf_core::payment_method::model::PaymentMethod;
use letaf_core::subscription::model::{Invoice, PlanKind, Subscription, SubscriptionStatus};

use crate::context::DesktopState;
use crate::format::money_br;
use crate::HTTP_CLIENT;
use crate::{
    MainWindow, PaymentMethodOption, PlanCardData, SubscriptionData,
    SubscriptionInvoiceRow,
};

use super::super::helpers::show_toast;
use super::pix::setup_pix_modal;

/// Plano do catálogo (vindo de GET /subscription/plans — cadastrado pelo
/// super admin). Espelha o `PlanPayload` do servidor.
#[derive(Deserialize, Default, Clone)]
struct CatalogPlan {
    id: String,
    name: String,
    amount: f64,
    period_months: i32,
    trial_days: i32,
    description: String,
    highlight_label: String,
    monthly_price: f64,
}

use super::card::setup_card;
use super::pix_auto::setup_pix_auto;
use super::payment_methods::setup_payment_method_crud;

pub(crate) fn setup_subscription(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    sync_cycle_done: Arc<Notify>,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
) {
    // Cache dos planos do catálogo (super admin). `reapply` o preenche a
    // cada fetch; `setup_choose_plan` o lê para assinar por id (o card só
    // carrega o id do plano, não os termos). Lock curto, sem `.await`.
    let catalog_cache: Arc<std::sync::Mutex<Vec<CatalogPlan>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    setup_refresh(ui, state, handle, auth_token.clone(), server_url.clone(), catalog_cache.clone());
    setup_choose_plan(ui, state, handle, sync_notify.clone(), catalog_cache.clone());
    setup_placeholders(ui);
    setup_pix_modal(ui, state, handle, auth_token.clone(), server_url.clone());
    // Recarrega a assinatura sempre que um ciclo de sync termina —
    // novas faturas/cobrança chegam via pull e devem refletir na UI
    // sem o operador clicar em nada.
    setup_sync_listener(ui, state, handle, sync_cycle_done, auth_token.clone(), server_url.clone(), catalog_cache.clone());
    // Toast inicial não-bloqueante: avisa overdue ou cobrança ≤ 3 dias.
    schedule_initial_notice(ui, state, handle);
    // Cartão recorrente (cobrança automática via gateway).
    setup_card(ui, state, handle, auth_token.clone(), server_url.clone(), sync_notify.clone());
    // Confirmação de troca de plano com recorrência ativa (cancela + troca).
    setup_plan_change_confirm(ui, state, handle, auth_token.clone(), server_url.clone(), sync_notify.clone());
    // Pix Automático (débito recorrente do Banco Central).
    setup_pix_auto(ui, state, handle, auth_token, server_url, sync_notify.clone());
    // Picker de forma de pagamento (seleção da forma ativa).
    setup_payment_method_crud(ui, state, handle, sync_notify);
}

/// Janela de antecedência para o aviso de "cobrança vencendo".
/// Centralizado para fácil ajuste futuro.
const UPCOMING_NOTICE_DAYS: i64 = 3;

fn schedule_initial_notice(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    handle.spawn(async move {
        // Pequeno delay para a UI montar antes do toast aparecer.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let cid = state.company_id();
        let today = Local::now().date_naive();
        let Ok(summary) = state.subscription_service.pending_summary(cid, today).await
        else {
            return;
        };
        let Some(message) = format_initial_notice(&summary) else {
            return;
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let tone = if summary.is_overdue { "error" } else { "info" };
                show_toast(&ui, &message, tone);
            }
        });
    });
}

fn format_initial_notice(
    summary: &letaf_core::subscription::model::PendingSummary,
) -> Option<String> {
    if summary.is_overdue {
        return Some(if summary.pending_invoice_count > 1 {
            format!(
                "Assinatura vencida · {} faturas em aberto",
                summary.pending_invoice_count
            )
        } else {
            "Assinatura vencida".to_string()
        });
    }
    match summary.days_until_next_charge {
        Some(d) if (0..=UPCOMING_NOTICE_DAYS).contains(&d) => Some(match d {
            0 => "Cobrança da assinatura vence hoje".to_string(),
            1 => "Cobrança da assinatura vence amanhã".to_string(),
            n => format!("Cobrança da assinatura vence em {n} dias"),
        }),
        _ => None,
    }
}

fn setup_sync_listener(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_cycle_done: Arc<Notify>,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
    catalog_cache: Arc<std::sync::Mutex<Vec<CatalogPlan>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    handle.spawn(async move {
        loop {
            sync_cycle_done.notified().await;
            reapply(&ui_weak, &state, &auth_token, &server_url, &catalog_cache).await;
        }
    });
}

fn setup_refresh(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
    catalog_cache: Arc<std::sync::Mutex<Vec<CatalogPlan>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_subscription_refresh(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        let catalog_cache = catalog_cache.clone();
        handle.spawn(async move {
            reapply(&ui_weak, &state, &auth_token, &server_url, &catalog_cache).await;
        });
    });
}

async fn reapply(
    ui_weak: &slint::Weak<MainWindow>,
    state: &DesktopState,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
    catalog_cache: &Arc<std::sync::Mutex<Vec<CatalogPlan>>>,
) {
    let cid = state.company_id();
    let sub = match state.subscription_service.find_current(cid).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            tracing::warn!("Subscription ainda não inicializada para company {}", cid);
            return;
        }
        Err(e) => {
            tracing::warn!("Subscription refresh falhou: {e}");
            return;
        }
    };
    let invoices = state
        .subscription_service
        .find_invoices(cid)
        .await
        .unwrap_or_default();
    let view = build_subscription_view(&sub);

    // Catálogo do super admin (online). Se vier, é a fonte dos cards;
    // vazio/offline → fallback nos planos fixos locais (offline-first §7).
    let catalog = fetch_catalog(auth_token, server_url).await;
    // Atualiza o cache para o `setup_choose_plan` assinar por id. Só
    // sobrescreve quando o fetch trouxe algo (offline mantém o último).
    if !catalog.is_empty() {
        if let Ok(mut guard) = catalog_cache.lock() {
            *guard = catalog.clone();
        }
    }
    let plan_cards: Vec<PlanCardData> = if !catalog.is_empty() {
        // Baseline = menor mensalidade entre planos de 1 mês (para "economize").
        let baseline = catalog
            .iter()
            .filter(|p| p.period_months == 1)
            .map(|p| p.monthly_price)
            .fold(f64::INFINITY, f64::min);
        let baseline = if baseline.is_finite() { baseline } else { 0.0 };
        catalog.iter().map(|p| catalog_plan_card(p, baseline)).collect()
    } else {
        let plans = state.subscription_service.available_plans();
        let monthly_baseline = plans
            .iter()
            .find(|p| p.kind == PlanKind::Monthly)
            .map(|p| p.monthly_price)
            .unwrap_or(0.0);
        plans
            .iter()
            .map(|p| plan_card(p, sub.plan_kind, monthly_baseline))
            .collect()
    };
    let invoice_rows: Vec<SubscriptionInvoiceRow> = invoices.iter().map(invoice_row).collect();

    // Badge da sidebar + card no Dashboard — ambos derivam do summary.
    let today = Local::now().date_naive();
    let summary = state
        .subscription_service
        .pending_summary(cid, today)
        .await
        .ok();
    let pending_count = summary.as_ref().map(|s| s.action_count as i32).unwrap_or(0);
    // Card de cobrança no Dashboard removido — `summary` segue só
    // para o badge da sidebar + toast inicial.
    let _ = summary;
    // Carrega catálogo persistido. Vazio = mostra só "PIX automático".
    let methods = state
        .payment_method_service
        .find_all(cid)
        .await
        .unwrap_or_default();
    let payment_methods = build_payment_methods(&sub, &methods);

    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_subscription_data(view);
            ui.set_subscription_plans(ModelRc::new(VecModel::from(plan_cards)));
            ui.set_subscription_invoices(ModelRc::new(VecModel::from(invoice_rows)));
            ui.set_subscription_pending_count(pending_count);
            ui.set_payment_methods(ModelRc::new(VecModel::from(payment_methods)));
        }
    });
}

/// Mescla as formas cadastradas com a opção fixa "PIX automático".
/// Selecionada = a `is_default` do banco; se ninguém é default, cai
/// no `payment_method` embutido na assinatura para retrocompat.
fn build_payment_methods(
    sub: &Subscription,
    methods: &[PaymentMethod],
) -> Vec<PaymentMethodOption> {
    // Cartão recorrente vinculado ao gateway: quando ativo, é a forma
    // selecionada e as demais opções ficam inativas.
    let card_active = sub.has_active_card();
    let any_default = methods.iter().any(|m| m.is_default);
    let fallback_kind = sub.payment_method.kind.as_str();

    let mut options: Vec<PaymentMethodOption> = Vec::new();

    // Cartão do gateway (cobrança automática) — id especial, não persiste
    // no catálogo. Aparece no topo quando há cartão vinculado.
    if card_active {
        let subtitle = if sub.payment_method.expiry.is_empty() {
            "Cobrança automática".to_string()
        } else {
            format!("Cobrança automática · expira {}", sub.payment_method.expiry)
        };
        options.push(PaymentMethodOption {
            kind: SharedString::from("card"),
            id: SharedString::from("card-gateway"),
            chip_label: SharedString::from("VISA"),
            title: SharedString::from(sub.payment_method.label.clone()),
            subtitle: SharedString::from(subtitle),
            is_active: true,
        });
    }

    // Formas cadastradas no catálogo. Com cartão do gateway ativo, todas
    // ficam inativas (a seleção é o cartão recorrente).
    options.extend(methods.iter().map(|m| {
        let title = match m.kind.as_str() {
            "card" => format!("{} {}", m.label, m.masked).trim().to_string(),
            _ => m.label.clone(),
        };
        let subtitle = if m.kind == "card" && !m.expiry.is_empty() {
            format!("expira {}", m.expiry)
        } else {
            String::new()
        };
        let chip_label = if m.kind == "pix" { "PIX" } else { "VISA" };
        PaymentMethodOption {
            kind: SharedString::from(m.kind.clone()),
            id: SharedString::from(m.base.id.to_string()),
            chip_label: SharedString::from(chip_label),
            title: SharedString::from(if title.is_empty() {
                m.label.clone()
            } else {
                title
            }),
            subtitle: SharedString::from(subtitle),
            is_active: !card_active && m.is_default,
        }
    }));

    // "PIX automático" sempre disponível — id especial, não persiste.
    options.push(PaymentMethodOption {
        kind: SharedString::from("pix"),
        id: SharedString::from("pix-instant"),
        chip_label: SharedString::from("PIX"),
        title: SharedString::from("PIX Automático"),
        subtitle: SharedString::default(),
        is_active: !card_active && !any_default && fallback_kind == "pix",
    });

    options
}

fn build_subscription_view(sub: &Subscription) -> SubscriptionData {
    // Assinatura de plano do catálogo (super admin) → mostra os termos
    // do snapshot (name/amount/period). Assinatura legada → cai no
    // catálogo fixo `plan_for(plan_kind)`.
    let plan = catalog_plan_view(sub).unwrap_or_else(|| plan_for(sub.plan_kind));

    let cycle_total = plan.total_per_charge;
    let charge_line_label = format!("Plano · {}", plan.label);
    let next_charge_display = sub
        .next_charge_date
        .map(format_next_charge)
        .unwrap_or_default();
    let payment_kind_chip = sub.payment_method.kind.clone();
    let payment_label = if sub.payment_method.label.is_empty() {
        "".to_string()
    } else {
        sub.payment_method.label.clone()
    };
    let payment_detail = if sub.payment_method.kind == "card" && !sub.payment_method.expiry.is_empty() {
        format!("Expira {}", sub.payment_method.expiry)
    } else if sub.payment_method.kind == "pix" {
        "PIX direto na sua conta".to_string()
    } else {
        String::new()
    };

    let (status_key, status_detail) = build_status_view(sub);

    SubscriptionData {
        status_key: SharedString::from(status_key),
        status_detail: SharedString::from(status_detail),
        plan_headline: SharedString::from("LETAF"),
        plan_suffix: SharedString::from(plan.label.clone()),
        plan_description: SharedString::from(format!(
            "{}. Sistema completo, sem limites de uso.",
            plan.description
        )),
        monthly_display: SharedString::from(money_br(plan.monthly_price)),
        cycle_display: SharedString::from(money_br(cycle_total)),
        next_charge_display: SharedString::from(next_charge_display),
        has_next_charge: sub.next_charge_date.is_some(),
        payment_method_kind: SharedString::from(payment_kind_chip),
        payment_method_label: SharedString::from(payment_label),
        payment_method_detail: SharedString::from(payment_detail),
        charge_line_label: SharedString::from(charge_line_label),
        charge_line_amount: SharedString::from(money_br(plan.monthly_price)),
        charge_total_display: SharedString::from(money_br(cycle_total)),
        current_plan_kind: SharedString::from(sub.plan_kind.as_str()),
        card_active: sub.has_active_card(),
        pix_auto_active: sub.has_active_pix_auto(),
    }
}

/// Busca os planos ATIVOS do catálogo (super admin). Vazio se offline/erro.
async fn fetch_catalog(
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) -> Vec<CatalogPlan> {
    let Some(token) = auth_token.read().await.clone() else {
        return Vec::new();
    };
    match HTTP_CLIENT
        .get(format!("{server_url}/subscription/plans"))
        .bearer_auth(&token)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r.json::<Vec<CatalogPlan>>().await.unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Converte o DTO do catálogo (JSON do servidor) no `Plan` do core para
/// assinar. Campos de auditoria são preenchidos localmente — o que importa
/// para o snapshot na assinatura são id/name/amount/period/trial.
fn catalog_to_plan(p: &CatalogPlan) -> Option<letaf_core::plan::model::Plan> {
    let id = uuid::Uuid::parse_str(&p.id).ok()?;
    let now = chrono::Utc::now().naive_utc();
    Some(letaf_core::plan::model::Plan {
        id,
        name: p.name.clone(),
        amount: p.amount,
        period_months: p.period_months,
        trial_days: p.trial_days,
        description: p.description.clone(),
        highlight_label: p.highlight_label.clone(),
        active: true,
        sort_order: 0,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    })
}

/// Monta o card a partir de um plano do catálogo (dinâmico).
fn catalog_plan_card(p: &CatalogPlan, baseline_monthly: f64) -> PlanCardData {
    let cycle_display = if p.period_months > 1 {
        format!("{} a cada {} meses", money_br(p.amount), p.period_months)
    } else {
        "Cobrado Mensalmente".to_string()
    };
    let savings = baseline_monthly - p.monthly_price;
    let savings_label = if savings > 0.5 {
        format!("ECONOMIZE {}/MÊS", money_br(savings))
    } else {
        String::new()
    };
    let trial_label = if p.trial_days > 0 {
        format!("{} dias grátis", p.trial_days)
    } else {
        String::new()
    };
    let tone = if p.highlight_label.is_empty() { "neutral" } else { "success" };
    PlanCardData {
        kind: SharedString::from(p.id.clone()),
        label: SharedString::from(p.name.clone()),
        description: SharedString::from(p.description.clone()),
        monthly_display: SharedString::from(money_br(p.monthly_price)),
        cycle_display: SharedString::from(cycle_display),
        savings_label: SharedString::from(savings_label),
        highlight_label: SharedString::from(p.highlight_label.clone()),
        is_current: false,
        tone: SharedString::from(tone),
        trial_label: SharedString::from(trial_label),
    }
}

fn plan_card(
    plan: &letaf_core::subscription::model::Plan,
    current: PlanKind,
    _monthly_baseline: f64,
) -> PlanCardData {
    let is_current = plan.kind == current;
    let cycle_display = match plan.kind {
        PlanKind::Monthly => "Cobrado Mensalmente".to_string(),
        PlanKind::Semestral => format!("{} a cada 6 meses", money_br(plan.total_per_charge)),
        PlanKind::Annual => format!("{} a cada 12 meses", money_br(plan.total_per_charge)),
    };
    let tone = match plan.kind {
        PlanKind::Monthly => "neutral",
        PlanKind::Semestral => "primary",
        PlanKind::Annual => "success",
    };
    PlanCardData {
        kind: SharedString::from(plan.kind.as_str()),
        label: SharedString::from(plan.label.clone()),
        description: SharedString::from(plan.description.clone()),
        monthly_display: SharedString::from(money_br(plan.monthly_price)),
        cycle_display: SharedString::from(cycle_display),
        savings_label: SharedString::from(plan.savings_label.clone()),
        highlight_label: SharedString::from(plan.highlight_label.clone()),
        is_current,
        tone: SharedString::from(tone),
        trial_label: SharedString::new(),
    }
}

fn invoice_row(inv: &Invoice) -> SubscriptionInvoiceRow {
    let status_label = match inv.status {
        letaf_core::subscription::model::InvoiceStatus::Paid => "Pago",
        letaf_core::subscription::model::InvoiceStatus::Pending => "Pendente",
        letaf_core::subscription::model::InvoiceStatus::Failed => "Falhou",
    };
    SubscriptionInvoiceRow {
        id: SharedString::from(inv.base.id.to_string()),
        issued_display: SharedString::from(inv.issued_at.format("%d/%m/%Y").to_string()),
        number: SharedString::from(inv.number.clone()),
        description: SharedString::from(inv.description.clone()),
        method_kind: SharedString::from(inv.method_kind.clone()),
        method_label: SharedString::from(inv.method_label.clone()),
        status_key: SharedString::from(inv.status.as_str()),
        status_label: SharedString::from(status_label),
        amount_display: SharedString::from(money_br(inv.amount)),
    }
}

fn setup_choose_plan(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    catalog_cache: Arc<std::sync::Mutex<Vec<CatalogPlan>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_subscription_choose_plan(move |kind_str| {
        let k = kind_str.as_str();
        // Planos do catálogo (id dinâmico do super admin) → assina por id,
        // com snapshot dos termos + trial. Os planos fixos legados
        // ("monthly"/"semestral"/"annual") seguem pelo fluxo antigo.
        if k != "monthly" && k != "semestral" && k != "annual" {
            let plan = catalog_cache
                .lock()
                .ok()
                .and_then(|guard| guard.iter().find(|p| p.id == k).cloned());
            let Some(catalog_plan) = plan.and_then(|p| catalog_to_plan(&p)) else {
                if let Some(ui) = ui_weak.upgrade() {
                    show_toast(&ui, "Plano indisponível. Atualize a página e tente de novo.", "error");
                }
                return;
            };
            let ui_weak = ui_weak.clone();
            let state = state.clone();
            let notify = sync_notify.clone();
            handle.spawn(async move {
                let cid = state.company_id();
                let today = Local::now().date_naive();
                match state
                    .subscription_service
                    .subscribe_to_plan(cid, &catalog_plan, today)
                    .await
                {
                    Ok(_) => {
                        notify.notify_one();
                        let label = format!("Plano alterado para {}", catalog_plan.name);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                show_toast(&ui, &label, "success");
                                ui.invoke_subscription_refresh();
                            }
                        });
                    }
                    Err(e) => {
                        // Recorrência ativa (cartão/PIX) → orientação clara,
                        // sem prefixo "Erro"; demais falhas → erro.
                        let (msg, tone) = match &e {
                            letaf_core::error::CoreError::Validation(m) => (m.clone(), "info"),
                            other => (format!("Erro ao assinar plano: {other}"), "error"),
                        };
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                show_toast(&ui, &msg, tone);
                            }
                        });
                    }
                }
            });
            return;
        }
        let plan = PlanKind::from_str(k);
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let today = Local::now().date_naive();
            // Recorrência ativa + plano diferente → confirma a troca
            // (cancela a recorrência e troca num passo só). Sem
            // recorrência (ou mesmo plano) → troca direta.
            if let Ok(Some(sub)) = state.subscription_service.find_current(cid).await {
                if sub.plan_kind != plan {
                    let recurrence = if sub.has_active_card() {
                        Some(("o cartão recorrente", "cadastrá-lo"))
                    } else if sub.has_active_pix_auto() {
                        Some(("o PIX Automático", "ativá-lo"))
                    } else {
                        None
                    };
                    if let Some((what, reactivate)) = recurrence {
                        let message = format!(
                            "Trocar para o plano {} vai cancelar {what}. Depois você precisará {reactivate} novamente com o novo valor. Deseja continuar?",
                            plan_word(plan)
                        );
                        let target_key = plan.as_str().to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                ui.set_plan_change_confirm_message(SharedString::from(message));
                                ui.set_plan_change_confirm_target(SharedString::from(target_key));
                                ui.set_plan_change_confirm_loading(false);
                                ui.set_plan_change_confirm_open(true);
                            }
                        });
                        return;
                    }
                }
            }
            match state.subscription_service.change_plan(cid, plan, today).await {
                Ok(updated) => {
                    notify.notify_one();
                    let label = format!("Plano alterado para {}", plan_word(updated.plan_kind));
                    let ui_weak_toast = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak_toast.upgrade() {
                            show_toast(&ui, &label, "success");
                            ui.invoke_subscription_refresh();
                        }
                    });
                }
                Err(e) => {
                    // Recorrência ativa (cartão/PIX Automático) → orientação
                    // clara como toast informativo, sem o prefixo "Erro".
                    let (msg, tone) = match &e {
                        letaf_core::error::CoreError::Validation(m) => (m.clone(), "info"),
                        other => (format!("Erro ao trocar plano: {other}"), "error"),
                    };
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            show_toast(&ui, &msg, tone);
                        }
                    });
                }
            }
        });
    });
}

/// Diálogo de confirmação da troca de plano com recorrência ativa.
/// No "Continuar": cancela a recorrência (gateway + local) e troca o
/// plano num passo só — o cliente só precisa reativar depois.
fn setup_plan_change_confirm(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    ui.on_plan_change_confirm_close(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_plan_change_confirm_open(false);
            ui.set_plan_change_confirm_loading(false);
        }
    });

    let ui_weak = ui.as_weak();
    let state_y = state.clone();
    let handle_y = handle.clone();
    ui.on_plan_change_confirm_yes(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let target = PlanKind::from_str(ui.get_plan_change_confirm_target().as_str());
        ui.set_plan_change_confirm_loading(true);
        let ui_weak = ui_weak.clone();
        let state = state_y.clone();
        let auth = auth_token.clone();
        let url = server_url.clone();
        let notify = sync_notify.clone();
        handle_y.spawn(async move {
            let cid = state.company_id();
            let today = Local::now().date_naive();
            let Some(token) = auth.read().await.clone() else {
                plan_confirm_error(&ui_weak, "Faça login para continuar".into());
                return;
            };
            let Ok(Some(sub)) = state.subscription_service.find_current(cid).await else {
                plan_confirm_error(&ui_weak, "Assinatura não encontrada".into());
                return;
            };
            // 1) Cancela a recorrência no gateway (server) + reflete local.
            if sub.has_active_card() {
                if let Err(m) = cancel_recurrence_remote(&url, "card", &token).await {
                    plan_confirm_error(&ui_weak, m);
                    return;
                }
                let _ = state.subscription_service.cancel_card(cid).await;
            } else if sub.has_active_pix_auto() {
                if let Err(m) = cancel_recurrence_remote(&url, "pix-auto", &token).await {
                    plan_confirm_error(&ui_weak, m);
                    return;
                }
                let _ = state.subscription_service.cancel_pix_auto(cid).await;
            }
            // 2) Troca o plano (agora passa na guarda do core).
            if let Err(e) = state.subscription_service.change_plan(cid, target, today).await {
                plan_confirm_error(&ui_weak, format!("Erro ao trocar plano: {e}"));
                return;
            }
            notify.notify_one();
            let label = format!(
                "Plano alterado para {} · cadastre novamente sua forma de pagamento recorrente",
                plan_word(target)
            );
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_plan_change_confirm_loading(false);
                    ui.set_plan_change_confirm_open(false);
                    show_toast(&ui, &label, "success");
                    ui.invoke_subscription_refresh();
                }
            });
        });
    });
}

/// DELETE da recorrência no server. `kind` = "card" | "pix-auto".
async fn cancel_recurrence_remote(
    server_url: &str,
    kind: &str,
    token: &str,
) -> Result<(), String> {
    let endpoint = format!("{}/subscription/{}", server_url, kind);
    let resp = HTTP_CLIENT
        .delete(&endpoint)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Falha de rede: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("Erro ao cancelar recorrência ({})", resp.status()))
    }
}

fn plan_confirm_error(ui_weak: &slint::Weak<MainWindow>, message: String) {
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_plan_change_confirm_loading(false);
            ui.set_plan_change_confirm_open(false);
            show_toast(&ui, &message, "error");
        }
    });
}

/// Callbacks "PDF" / "Baixar .zip" mostram toast "Em breve" até o
/// gateway real existir. O toggle do picker e a escolha de forma de
/// pagamento já têm efeito visível imediato.
fn setup_placeholders(ui: &MainWindow) {
    // Toggle do painel inline "Trocar forma de pagamento".
    let ui_weak = ui.as_weak();
    ui.on_subscription_toggle_payment_picker(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_payment_picker_open(!ui.get_payment_picker_open());
        }
    });

    // pick_payment_method e open_add_payment_method são tratados em
    // `setup_payment_method_crud` para ter acesso ao service.

    let ui_weak = ui.as_weak();
    ui.on_subscription_download_invoice(move |_id| {
        if let Some(ui) = ui_weak.upgrade() {
            show_toast(&ui, "Geração de PDF · em breve", "info");
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_subscription_download_all(move || {
        if let Some(ui) = ui_weak.upgrade() {
            show_toast(&ui, "Download das faturas (.zip) · em breve", "info");
        }
    });
}

// ── Helpers ──────────────────────────────────────────────────────

/// Deriva um `Plan` de exibição a partir do snapshot da assinatura de
/// catálogo. `None` se for assinatura legada (sem `plan_id`).
fn catalog_plan_view(sub: &Subscription) -> Option<letaf_core::subscription::model::Plan> {
    if !sub.is_catalog_plan() {
        return None;
    }
    let months = sub.plan_period_months.max(1);
    let monthly = sub.plan_amount / months as f64;
    Some(letaf_core::subscription::model::Plan {
        kind: sub.plan_kind,
        label: sub.plan_name.clone(),
        monthly_price: monthly,
        total_per_charge: sub.plan_amount,
        savings_label: String::new(),
        highlight_label: String::new(),
        description: if months > 1 {
            format!("Cobrado a cada {months} meses")
        } else {
            "Cobrado mensalmente · Cancele quando quiser".to_string()
        },
    })
}

fn plan_for(kind: PlanKind) -> letaf_core::subscription::model::Plan {
    // Espelho do catálogo do service. Centralizar no Rust handler é
    // ok porque o catálogo é pequeno e a UI já mostra a partir dele.
    let monthly = 200.0_f64;
    let semestral_monthly = 190.0_f64;
    let annual_monthly = 180.0_f64;
    use letaf_core::subscription::model::Plan;
    match kind {
        PlanKind::Monthly => Plan {
            kind,
            label: "Mensal".into(),
            monthly_price: monthly,
            total_per_charge: monthly,
            savings_label: String::new(),
            highlight_label: String::new(),
            description: "Cobrado todo mês · Cancele quando quiser".into(),
        },
        PlanKind::Semestral => Plan {
            kind,
            label: "Semestral".into(),
            monthly_price: semestral_monthly,
            total_per_charge: semestral_monthly * 6.0,
            savings_label: format!("ECONOMIZE R$ {}/MÊS", (monthly - semestral_monthly) as i64),
            highlight_label: String::new(),
            description: format!("Cobrado a cada 6 meses · R$ {}/mês", semestral_monthly as i64),
        },
        PlanKind::Annual => Plan {
            kind,
            label: "Anual".into(),
            monthly_price: annual_monthly,
            total_per_charge: annual_monthly * 12.0,
            savings_label: format!("ECONOMIZE R$ {}/MÊS", (monthly - annual_monthly) as i64),
            highlight_label: "MELHOR VALOR".into(),
            description: format!("Cobrado 1× por ano · R$ {}/mês", annual_monthly as i64),
        },
    }
}

/// Texto auxiliar do card: status + dias em atraso quando overdue.
/// `next_charge_date` é referência: se está no passado, calculamos
/// quantos dias atrás venceu para mostrar "há N dias".
fn build_status_view(sub: &Subscription) -> (&'static str, String) {
    match sub.status {
        SubscriptionStatus::Active => ("active", String::new()),
        SubscriptionStatus::Cancelled => ("cancelled", "Assinatura cancelada".to_string()),
        SubscriptionStatus::Overdue => {
            let today = Local::now().date_naive();
            let detail = sub
                .next_charge_date
                .map(|d| {
                    let days = (today - d).num_days().max(0);
                    if days == 0 {
                        "Vencida hoje".to_string()
                    } else {
                        format!("Vencida há {days} dia{}", if days > 1 { "s" } else { "" })
                    }
                })
                .unwrap_or_else(|| "Vencida".to_string());
            ("overdue", detail)
        }
    }
}

fn plan_word(kind: PlanKind) -> &'static str {
    match kind {
        PlanKind::Monthly => "Mensal",
        PlanKind::Semestral => "Semestral",
        PlanKind::Annual => "Anual",
    }
}

fn format_next_charge(d: NaiveDate) -> String {
    format!("{:02}/{}", d.day(), month_abbr_pt(d.month()))
}

fn month_abbr_pt(m: u32) -> &'static str {
    match m {
        1 => "jan", 2 => "fev", 3 => "mar", 4 => "abr",
        5 => "mai", 6 => "jun", 7 => "jul", 8 => "ago",
        9 => "set", 10 => "out", 11 => "nov", 12 => "dez",
        _ => "—",
    }
}

