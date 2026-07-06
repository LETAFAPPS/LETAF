use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, SharedString};
use uuid::Uuid;


use crate::MainWindow;

use super::super::helpers::show_toast;
use super::state::PdvState;
use super::view::apply_state_to_ui;

pub(crate) fn slint_row_count<T: Clone + 'static>(model: &slint::ModelRc<T>) -> usize {
    use slint::Model;
    model.row_count()
}
pub(crate) fn slint_row_data<T: Clone + 'static>(model: &slint::ModelRc<T>, i: usize) -> Option<T> {
    use slint::Model;
    model.row_data(i)
}

pub(crate) fn setup_inc_line(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_inc_line(move |line_id| {
        let Ok(uuid) = Uuid::parse_str(line_id.as_str()) else { return };
        if let Ok(mut g) = pdv.lock() {
            if let Some(line) = g.cart.iter_mut().find(|l| l.line_id == uuid) {
                line.qty += 1.0;
            }
        }
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

pub(crate) fn setup_dec_line(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_dec_line(move |line_id| {
        let Ok(uuid) = Uuid::parse_str(line_id.as_str()) else { return };
        if let Ok(mut g) = pdv.lock() {
            if let Some(pos) = g.cart.iter().position(|l| l.line_id == uuid) {
                if g.cart[pos].qty > 1.0 {
                    g.cart[pos].qty -= 1.0;
                } else {
                    g.cart.remove(pos);
                }
            }
        }
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

pub(crate) fn setup_remove_line(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_remove_line(move |line_id| {
        let Ok(uuid) = Uuid::parse_str(line_id.as_str()) else { return };
        if let Ok(mut g) = pdv.lock() {
            g.cart.retain(|l| l.line_id != uuid);
        }
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

pub(crate) fn setup_clear_cart(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_clear_cart(move || {
        if let Ok(mut g) = pdv.lock() {
            // Limpa o carrinho INTEIRO: itens + desconto + adicional +
            // valor pago. As formas de pagamento/inputs são zeradas na UI.
            g.cart.clear();
            g.discount_value = 0.0;
            g.additional_value = 0.0;
            g.amount_paid = 0.0;
        }
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_pdv_discount_input(SharedString::default());
            ui.set_pdv_additional_input(SharedString::default());
            ui.set_pdv_amount_paid_input(SharedString::default());
            ui.set_pdv_payment_method(SharedString::default());
            ui.set_pdv_secondary_payment(SharedString::default());
            apply_state_to_ui(&ui, &pdv);
            show_toast(&ui, "Carrinho limpo", "warning");
        }
    });
}

