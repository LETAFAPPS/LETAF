use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use letaf_core::order::model::DeliveryType;
use letaf_core::order::service::OrderItemInput;

use crate::context::DesktopState;
use crate::{MainWindow, PdvAddressRow};

use super::super::helpers::show_toast;
use super::state::PdvState;
use super::customer::apply_wallet_to_ui;
use super::view::apply_state_to_ui;

/// `pdv-finalize` — agora dispara DIRETO a criação do pedido a partir
/// do estado integrado no carrinho (sem modal intermediário).
/// Validações:
///   - Carrinho não vazio.
///   - Se `sale-type == "delivery"`, endereço (rua + nº + bairro) obrigatório.
///   - `payment_method` é validado pelo service.
pub(crate) fn setup_finalize(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    pdv: Arc<Mutex<PdvState>>,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_pdv_finalize(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };

        // Caixa aberto é pré-requisito do PDV — o `session_id` vive
        // no `cash_summary` populado pelo `cash_refresh`. Sem ele,
        // a venda nem entra na sessão (movimento não seria atribuído).
        let session_id_str = ui_ref.get_cash_summary().session_id.to_string();
        let session_id = Uuid::parse_str(&session_id_str).ok();
        if session_id.is_none() {
            ui_ref.set_pdv_finalize_error(SharedString::from(
                "Abra o caixa antes de finalizar a venda.",
            ));
            return;
        }

        let sale_type = ui_ref.get_pdv_sale_type().to_string();
        let primary = ui_ref.get_pdv_payment_method().to_string();
        let card_type = ui_ref.get_pdv_payment_card_type().to_string();
        let secondary = ui_ref.get_pdv_secondary_payment().to_string();
        let customer_id_str = ui_ref.get_pdv_customer_id().to_string();
        let street = ui_ref.get_pdv_delivery_street().to_string();
        let number = ui_ref.get_pdv_delivery_number().to_string();
        let neigh = ui_ref.get_pdv_delivery_neighborhood().to_string();

        let (items, discount, additional, total, amount_paid) = {
            let Ok(g) = pdv.lock() else { return };
            if g.cart.is_empty() {
                ui_ref.set_pdv_finalize_error(SharedString::from("Carrinho vazio."));
                return;
            }
            let items: Vec<OrderItemInput> = g.cart.iter().map(|line| OrderItemInput {
                product_id: line.product_id,
                product_name: line.name.clone(),
                quantity: line.qty,
                unit_price: line.unit_price,
                notes: None,
                addons_json: line.addons_json.clone(),
            }).collect();
            (items, g.discount_value, g.additional_value, g.total(), g.amount_paid)
        };
        if sale_type == "delivery"
            && (street.trim().is_empty() || number.trim().is_empty() || neigh.trim().is_empty())
        {
            ui_ref.set_pdv_finalize_error(SharedString::from(
                "Para Entrega preencha Rua, Nº e Bairro."
            ));
            return;
        }

        // Forma de pagamento obrigatória (sem seleção por padrão).
        // Mensagem em pt-BR — evita o erro técnico do core ("Unknown
        // payment method ''").
        if primary.trim().is_empty() {
            ui_ref.set_pdv_finalize_error(SharedString::from(
                "Selecione uma forma de pagamento."
            ));
            show_toast(&ui_ref, "Selecione uma forma de pagamento.", "warning");
            return;
        }

        // Resolve `payment_method` final + nota explicativa:
        // - Cartão → "credit" ou "debit" conforme sub-seleção.
        // - Dinheiro com valor < total → exige `secondary` selecionado
        //   (Crédito/Débito/Pix) e marca o método final como o
        //   secundário (já que o dinheiro foi pagamento parcial).
        //   Nota registra "[Pago R$ X em dinheiro + R$ Y em <forma>]".
        // - Pix → "pix".
        let (payment_method_final, extra_note) = match primary.as_str() {
            "card" => (card_type.clone(), String::new()),
            "pix" => ("pix".to_string(), String::new()),
            "wallet" => {
                // Pré-validação cliente-side (defense-in-depth, mas
                // service também valida). Bloqueia cliente vazio,
                // sem conta ou saldo insuficiente antes de criar o
                // pedido — evita inconsistência onde a venda existiria
                // sem cobrança.
                if customer_id_str.trim().is_empty() {
                    ui_ref.set_pdv_finalize_error(SharedString::from(
                        "Carteira exige cliente vinculado."
                    ));
                    return;
                }
                let account_id_str = ui_ref.get_pdv_wallet_account_id().to_string();
                if account_id_str.is_empty() {
                    ui_ref.set_pdv_finalize_error(SharedString::from(
                        "Cliente sem carteira aberta. Abra a carteira no detalhe do cliente primeiro."
                    ));
                    return;
                }
                let available = ui_ref.get_pdv_wallet_available_amount() as f64;
                if available + 0.005 < total {
                    ui_ref.set_pdv_finalize_error(SharedString::from(
                        "Saldo + limite da carteira não cobre o total."
                    ));
                    return;
                }
                ("wallet".to_string(), String::new())
            }
            "cash" => {
                if amount_paid >= total {
                    ("cash".to_string(), String::new())
                } else if amount_paid > 0.0 && !secondary.is_empty() {
                    let remaining = total - amount_paid;
                    let note = format!(
                        "[Pago R$ {:.2} em dinheiro + R$ {:.2} em {}]",
                        amount_paid, remaining,
                        match secondary.as_str() {
                            "credit" => "crédito",
                            "debit" => "débito",
                            "pix" => "pix",
                            _ => &secondary,
                        }
                    );
                    (secondary.clone(), note)
                } else if amount_paid == 0.0 {
                    // Operador clicou Dinheiro mas não digitou — assume "cash" cheio.
                    ("cash".to_string(), String::new())
                } else {
                    ui_ref.set_pdv_finalize_error(SharedString::from(
                        "Valor pago abaixo do total — selecione a forma do restante."
                    ));
                    return;
                }
            }
            _ => (primary.clone(), String::new()),
        };

        let delivery_type = if sale_type == "delivery" {
            DeliveryType::Delivery
        } else {
            DeliveryType::Pickup
        };
        let base_notes = if sale_type == "delivery" {
            format!("[Entrega] {}, {}, {}", street.trim(), number.trim(), neigh.trim())
        } else {
            "[Balcão]".to_string()
        };
        let final_notes = if extra_note.is_empty() {
            Some(base_notes)
        } else {
            Some(format!("{base_notes} {extra_note}"))
        };
        let payment_method = payment_method_final;
        let customer_id = Uuid::parse_str(&customer_id_str).unwrap_or(Uuid::nil());

        // Capturado fora do spawn pra evitar passar a Slint `Weak`
        // dentro do executor antes do invoke_from_event_loop.
        let wallet_account_uuid: Option<Uuid> = if payment_method == "wallet" {
            Uuid::parse_str(ui_ref.get_pdv_wallet_account_id().as_str()).ok()
        } else {
            None
        };

        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let pdv = pdv.clone();
        let notify = sync_notify.clone();
        let payment_method_clone = payment_method.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let result = state.order_service.create_pdv(
                cid,
                customer_id,
                items,
                discount,
                additional,
                delivery_type,
                Some(payment_method),
                final_notes,
                session_id,
            ).await;

            // Se o pagamento foi via carteira, cobra o saldo do
            // cliente AGORA. Ordem proposital: pedido primeiro
            // (fonte de verdade); cobrança depois. Se a cobrança
            // falhar, loga e toa avisa — venda permanece registrada
            // e o operador concilia manualmente.
            let wallet_warning = match (&result, wallet_account_uuid) {
                (Ok(order), Some(acc_id)) if payment_method_clone == "wallet" => {
                    match state
                        .wallet_service
                        .charge_order(cid, acc_id, order.total, order.base.id)
                        .await
                    {
                        Ok(_) => None,
                        Err(e) => {
                            tracing::warn!(
                                "PDV order {} criada mas charge_order falhou: {e}",
                                order.base.id
                            );
                            Some("Venda registrada, mas a cobrança da carteira falhou. Concilie manualmente.".to_string())
                        }
                    }
                }
                _ => None,
            };

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(order) => {
                        if let Ok(mut g) = pdv.lock() {
                            g.cart.clear();
                            g.discount_value = 0.0;
                            g.additional_value = 0.0;
                            g.amount_paid = 0.0;
                            g.current_customer_addresses.clear();
                        }
                        // Limpa campos do carrinho.
                        ui.set_pdv_finalize_error(SharedString::default());
                        ui.set_pdv_delivery_street(SharedString::default());
                        ui.set_pdv_delivery_number(SharedString::default());
                        ui.set_pdv_delivery_neighborhood(SharedString::default());
                        ui.set_pdv_customer_id(SharedString::default());
                        ui.set_pdv_customer_name(SharedString::default());
                        ui.set_pdv_discount_input(SharedString::default());
                        ui.set_pdv_additional_input(SharedString::default());
                        ui.set_pdv_amount_paid_input(SharedString::default());
                        ui.set_pdv_secondary_payment(SharedString::default());
                        // Limpa a forma de pagamento — próxima venda começa sem seleção.
                        ui.set_pdv_payment_method(SharedString::default());
                        ui.set_pdv_customer_addresses(
                            ModelRc::new(VecModel::from(Vec::<PdvAddressRow>::new())),
                        );
                        // Modal pós-venda.
                        ui.set_pdv_sold_order_id(SharedString::from(order.base.id.to_string()));
                        ui.set_pdv_sold_order_number(SharedString::from(format!("{:04}", order.number)));
                        ui.set_pdv_sold_total(SharedString::from(format!("R$ {:.2}", order.total)));
                        ui.set_pdv_show_sold(true);
                        apply_state_to_ui(&ui, &pdv);
                        notify.notify_one();
                        ui.invoke_refresh_orders();
                        // Atualiza o dashboard de Caixa com a nova venda
                        // (movimento foi lançado pelo service).
                        ui.invoke_cash_refresh();
                        // Limpa o estado da carteira no PDV — próximo
                        // cliente recarrega.
                        apply_wallet_to_ui(&ui, None);
                        if let Some(msg) = wallet_warning {
                            show_toast(&ui, &msg, "error");
                        } else {
                            show_toast(&ui, "Venda Registrada", "success");
                        }
                    }
                    Err(e) => {
                        // Mensagem para o operador em pt-BR, sem o prefixo
                        // técnico do erro (ex.: "Validation:"). Erros de
                        // validação já carregam texto pt-BR; demais erros
                        // recebem mensagem genérica. Log mantém o erro cru.
                        let msg = match &e {
                            letaf_core::error::CoreError::Validation(m) => m.clone(),
                            _ => "Não foi possível finalizar a venda. Tente novamente.".to_string(),
                        };
                        tracing::warn!("PDV finalize falhou: {e}");
                        ui.set_pdv_finalize_error(SharedString::from(msg.clone()));
                        show_toast(&ui, &msg, "error");
                    }
                }
            });
        });
    });
}

