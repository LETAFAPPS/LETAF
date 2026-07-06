use std::sync::Arc;

use chrono::{Datelike, Local, NaiveDate};

use letaf_core::customer::model::Customer;



/// Estado de navegação do calendário compartilhado entre callbacks.
/// `(year, month, selected_day)` — selected_day = `None` quando nada
/// está clicado.
#[derive(Clone)]
pub(crate) struct CalState {
    pub(crate) year: i32,
    pub(crate) month: u32,
    pub(crate) selected_day: Option<u32>,
}

impl CalState {
    pub(crate) fn today() -> Self {
        let t = Local::now().date_naive();
        Self {
            year: t.year(),
            month: t.month(),
            selected_day: None,
        }
    }
}

pub(crate) type CalStateHandle = Arc<std::sync::Mutex<CalState>>;

/// Estado do calendário POPUP do campo Vencimento (Fase 13).
/// Compartilhado entre callbacks `due-cal-*` para preservar o mês
/// visível entre cliques (≠ do calendário mensal da tela completa).
#[derive(Clone, Copy)]
pub(crate) struct DueCalState {
    pub(crate) year: i32,
    pub(crate) month: u32,
    pub(crate) selected: Option<NaiveDate>,
}

impl DueCalState {
    pub(crate) fn today() -> Self {
        let t = Local::now().date_naive();
        Self {
            year: t.year(),
            month: t.month(),
            selected: Some(t),
        }
    }
}

pub(crate) type DueCalStateHandle = Arc<std::sync::Mutex<DueCalState>>;
pub(crate) type CustomersHandle = Arc<std::sync::Mutex<Vec<Customer>>>;

