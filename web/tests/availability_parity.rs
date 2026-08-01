//! Paridade entre `web::availability` (dica de UI no cliente Leptos/wasm) e
//! `core::availability` (fonte da verdade do backend, usada no checkout).
//! A lógica é DUPLICADA de propósito: reusar o core no cliente puxaria
//! `chrono`/`rust_decimal` para o bundle wasm. Este teste (dev-dependency,
//! roda nativo e fora do bundle) trava a divergência — se a regra de
//! disponibilidade mudar num lado e não no outro, quebra.
//!
//! Modelo: `web/tests/discount_parity.rs`.
//!
//! Diferenças ESTRUTURAIS conhecidas (não são divergências de regra):
//! - o core recebe `(day, mins)` já resolvidos no fuso da loja
//!   (`core::availability::local_now` + `Company::utc_offset_minutes`);
//!   o web recebe `Option<(day, mins)>` do relógio do NAVEGADOR e trata
//!   `None` (SSR/pré-hidratação) como disponível/aberto. Para o MESMO
//!   `(day, mins)` as duas implementações devem concordar sempre — é
//!   exatamente isso que este teste verifica.
//! - a ORIGEM do relógio diverge por design: fuso fixo da empresa no
//!   backend vs. relógio local do cliente no web (§11: o cliente é
//!   não-confiável, por isso o backend reconfere).

use letaf_core::availability as core_av;
use letaf_core::business_hours::model::BusinessHours;
use letaf_web::api::BusinessHoursEntry;
use letaf_web::availability as web_av;
use uuid::Uuid;

/// Especificação de um dia: `(dia_da_semana, abertura, fechamento, aberto?)`.
type DaySpec<'a> = (i32, &'a str, &'a str, bool);

/// Dias da semana varridos, incluindo valores FORA da faixa 0..=6 para
/// garantir que os dois lados tratam dia desconhecido do mesmo jeito.
const SWEPT_DAYS: [i32; 10] = [-1, 0, 1, 2, 3, 4, 5, 6, 7, 99];

fn core_hours(spec: &[DaySpec]) -> Vec<BusinessHours> {
    spec.iter()
        .map(|(day, open, close, is_open)| {
            BusinessHours::new(Uuid::nil(), *day, (*open).into(), (*close).into(), *is_open)
        })
        .collect()
}

fn web_hours(spec: &[DaySpec]) -> Vec<BusinessHoursEntry> {
    spec.iter()
        .map(|(day, open, close, is_open)| BusinessHoursEntry {
            day_of_week: *day,
            open_time: (*open).into(),
            close_time: (*close).into(),
            is_open: *is_open,
        })
        .collect()
}

/// Roda os DOIS lados no mesmo instante, exige que concordem e devolve o
/// resultado comum. Toda asserção de comportamento passa por aqui, então
/// cada caso do teste também é um caso de paridade.
fn store_open(spec: &[DaySpec], store_override: &str, day: i32, mins: i32) -> bool {
    let core = core_av::is_store_open(&core_hours(spec), store_override, day, mins);
    let web = web_av::is_store_open_now(&web_hours(spec), store_override, Some((day, mins)));
    assert_eq!(
        core, web,
        "divergência LOJA: spec={spec:?} override={store_override:?} day={day} mins={mins} \
         core={core} web={web}",
    );
    core
}

/// Idem para o `availability_schedule` do produto.
fn product_available(schedule: Option<&str>, day: i32, mins: i32) -> bool {
    let core = core_av::is_product_available(schedule, day, mins);
    let web = web_av::is_available_now(schedule, Some((day, mins)));
    assert_eq!(
        core, web,
        "divergência PRODUTO: schedule={schedule:?} day={day} mins={mins} \
         core={core} web={web}",
    );
    core
}

/// Varredura exaustiva: todos os dias (inclusive inválidos) × todos os
/// minutos do dia. Pega qualquer divergência de borda sem depender de o
/// teste "adivinhar" o minuto certo.
fn sweep_store(spec: &[DaySpec], store_override: &str) {
    for day in SWEPT_DAYS {
        for mins in 0..24 * 60 {
            store_open(spec, store_override, day, mins);
        }
    }
}

fn sweep_product(schedule: Option<&str>) {
    for day in SWEPT_DAYS {
        for mins in 0..24 * 60 {
            product_available(schedule, day, mins);
        }
    }
}

/// Monta um `availability_schedule` JSON com um dia só.
fn sched(day: i32, open: &str, close: &str, active: bool) -> String {
    format!(r#"[{{"day":{day},"open":"{open}","close":"{close}","active":{active}}}]"#)
}

// ---------------------------------------------------------------------------
// Loja (business hours)
// ---------------------------------------------------------------------------

#[test]
fn loja_janela_normal_e_bordas() {
    // Segunda (1), 08:00 (480) → 18:00 (1080). Janela `[open, close)`.
    let spec: &[DaySpec] = &[(1, "08:00", "18:00", true)];
    sweep_store(spec, "none");

    assert!(!store_open(spec, "none", 1, 479)); // 07:59 — um minuto antes
    assert!(store_open(spec, "none", 1, 480)); // 08:00 — exatamente na abertura
    assert!(store_open(spec, "none", 1, 481)); // 08:01
    assert!(store_open(spec, "none", 1, 1079)); // 17:59 — um minuto antes
    assert!(!store_open(spec, "none", 1, 1080)); // 18:00 — fechamento é EXCLUSIVO
    assert!(!store_open(spec, "none", 1, 1081)); // 18:01
    assert!(!store_open(spec, "none", 1, 0)); // 00:00
}

#[test]
fn loja_override_todas_as_variantes() {
    // Variantes válidas em `company::service::set_store_override`:
    // "none" | "open" | "closed". Qualquer outro valor cai no ramo neutro.
    let spec: &[DaySpec] = &[(1, "08:00", "18:00", true)];

    // "open" força aberto mesmo em dia/hora fechados.
    sweep_store(spec, "open");
    assert!(store_open(spec, "open", 3, 0)); // quarta 00:00, sem cadastro
    assert!(store_open(&[], "open", 1, 600)); // sem horários cadastrados

    // "closed" força fechado mesmo dentro da janela.
    sweep_store(spec, "closed");
    assert!(!store_open(spec, "closed", 1, 600)); // segunda 10:00
    assert!(!store_open(&[], "closed", 1, 600));

    // "none" e QUALQUER string desconhecida seguem o horário cadastrado —
    // inclusive variações de caixa: o casamento é sensível a maiúsculas.
    for neutro in ["none", "", "auto", "aberto", "OPEN", "Closed", "open "] {
        sweep_store(spec, neutro);
        assert!(store_open(spec, neutro, 1, 600)); // 10:00 dentro da janela
        assert!(!store_open(spec, neutro, 1, 1200)); // 20:00 fora
    }
}

#[test]
fn loja_sem_horarios_ou_dia_sem_cadastro() {
    // Nenhum horário cadastrado → ABERTA (degradação graciosa nos dois lados).
    sweep_store(&[], "none");
    assert!(store_open(&[], "none", 0, 0));

    // Há cadastro, mas não para o dia consultado → FECHADA.
    let spec: &[DaySpec] = &[(1, "08:00", "18:00", true)];
    assert!(!store_open(spec, "none", 2, 600)); // terça sem cadastro
    assert!(!store_open(spec, "none", 0, 600)); // domingo sem cadastro
    assert!(!store_open(spec, "none", 7, 600)); // dia inválido

    // Dia cadastrado como fechado (`is_open = false`) → FECHADA o dia todo.
    let fechado: &[DaySpec] = &[(1, "08:00", "18:00", false)];
    sweep_store(fechado, "none");
    assert!(!store_open(fechado, "none", 1, 600));
}

#[test]
fn loja_janela_atravessa_meia_noite() {
    // Bar noturno: sexta (5) 18:00 (1080) → 02:00 (120) do dia seguinte.
    // `close <= open` cruza 00:00 — suportado nas DUAS implementações.
    let spec: &[DaySpec] = &[(5, "18:00", "02:00", true)];
    sweep_store(spec, "none");

    assert!(!store_open(spec, "none", 5, 1079)); // 17:59
    assert!(store_open(spec, "none", 5, 1080)); // 18:00 — abertura
    assert!(store_open(spec, "none", 5, 1439)); // 23:59
    assert!(store_open(spec, "none", 5, 0)); // 00:00
    assert!(store_open(spec, "none", 5, 119)); // 01:59
    assert!(!store_open(spec, "none", 5, 120)); // 02:00 — fechamento exclusivo
    assert!(!store_open(spec, "none", 5, 720)); // 12:00

    // Atenção (comportamento REAL, idêntico nos dois lados): a janela é
    // avaliada contra o dia CORRENTE, não contra o dia em que ela começou.
    // Sábado 01:00 fica fechado se o sábado não tiver cadastro próprio.
    assert!(!store_open(spec, "none", 6, 60));
}

#[test]
fn loja_janela_24h_quando_open_igual_close() {
    // `close == open` cai no ramo da virada → aberto 24h.
    let spec: &[DaySpec] = &[(1, "08:00", "08:00", true)];
    sweep_store(spec, "none");
    assert!(store_open(spec, "none", 1, 0));
    assert!(store_open(spec, "none", 1, 479));
    assert!(store_open(spec, "none", 1, 480));
    assert!(store_open(spec, "none", 1, 1439));

    let meia_noite: &[DaySpec] = &[(1, "00:00", "00:00", true)];
    sweep_store(meia_noite, "none");
    assert!(store_open(meia_noite, "none", 1, 733));
}

#[test]
fn loja_horario_invalido_usa_fallback() {
    // `open` inválido → 0; `close` inválido → 24*60. Fallbacks idênticos.
    let open_ruim: &[DaySpec] = &[(1, "xx:yy", "18:00", true)];
    sweep_store(open_ruim, "none");
    assert!(store_open(open_ruim, "none", 1, 0)); // vale como 00:00
    assert!(!store_open(open_ruim, "none", 1, 1080));

    let close_ruim: &[DaySpec] = &[(1, "08:00", "99:99", true)];
    sweep_store(close_ruim, "none");
    assert!(store_open(close_ruim, "none", 1, 1439)); // vale como 24:00
    assert!(!store_open(close_ruim, "none", 1, 479));

    // Formatos degenerados: sem ":", vazio, hora/minuto fora da faixa.
    for (open, close) in [("", ""), ("8h", "18h"), ("24:00", "25:00"), ("08:60", "18:00")] {
        let spec: &[DaySpec] = &[(1, open, close, true)];
        sweep_store(spec, "none");
        assert!(store_open(spec, "none", 1, 600)); // 00:00→24:00 = sempre aberta
    }
}

#[test]
fn loja_semana_completa_e_dias_duplicados() {
    let semana: &[DaySpec] = &[
        (0, "00:00", "00:00", false),
        (1, "08:00", "18:00", true),
        (2, "08:00", "12:00", true),
        (3, "12:00", "23:59", true),
        (4, "18:00", "02:00", true),
        (5, "18:00", "03:30", true),
        (6, "10:00", "16:00", false),
    ];
    sweep_store(semana, "none");
    sweep_store(semana, "open");
    sweep_store(semana, "closed");

    // Dia duplicado: a PRIMEIRA entrada vence nos dois lados (`find`).
    let dup: &[DaySpec] = &[(1, "08:00", "12:00", true), (1, "14:00", "18:00", true)];
    sweep_store(dup, "none");
    assert!(store_open(dup, "none", 1, 600)); // 10:00 → 1ª entrada
    assert!(!store_open(dup, "none", 1, 900)); // 15:00 → 2ª entrada ignorada
}

// ---------------------------------------------------------------------------
// Produto (availability_schedule)
// ---------------------------------------------------------------------------

#[test]
fn produto_sem_schedule_ou_json_invalido() {
    // Ausente, vazio ou ilegível → DISPONÍVEL (degradação graciosa).
    for schedule in [None, Some(""), Some("lixo-invalido"), Some("{}"), Some("[{\"day\":1}]")] {
        sweep_product(schedule);
        assert!(product_available(schedule, 1, 600));
    }

    // Array VAZIO é JSON válido → nenhuma entrada para o dia →
    // INDISPONÍVEL. Quirk real, idêntico nos dois lados (difere de `""`).
    sweep_product(Some("[]"));
    assert!(!product_available(Some("[]"), 1, 600));
}

#[test]
fn produto_janela_normal_e_bordas() {
    let s = sched(1, "08:00", "12:00", true); // 480 → 720
    sweep_product(Some(&s));

    assert!(!product_available(Some(&s), 1, 479)); // 07:59
    assert!(product_available(Some(&s), 1, 480)); // 08:00 — abertura
    assert!(product_available(Some(&s), 1, 719)); // 11:59
    assert!(!product_available(Some(&s), 1, 720)); // 12:00 — exclusivo
    assert!(!product_available(Some(&s), 1, 721)); // 12:01
}

#[test]
fn produto_dia_sem_entrada_ou_inativo() {
    let s = sched(1, "08:00", "12:00", true);
    assert!(!product_available(Some(&s), 2, 600)); // terça sem entrada
    assert!(!product_available(Some(&s), 0, 600)); // domingo sem entrada
    assert!(!product_available(Some(&s), 7, 600)); // dia inválido

    let inativo = sched(1, "08:00", "12:00", false);
    sweep_product(Some(&inativo));
    assert!(!product_available(Some(&inativo), 1, 600)); // dia inativo
}

#[test]
fn produto_janela_atravessa_meia_noite() {
    let s = sched(5, "18:00", "02:00", true);
    sweep_product(Some(&s));

    assert!(!product_available(Some(&s), 5, 1079)); // 17:59
    assert!(product_available(Some(&s), 5, 1080)); // 18:00
    assert!(product_available(Some(&s), 5, 0)); // 00:00
    assert!(product_available(Some(&s), 5, 119)); // 01:59
    assert!(!product_available(Some(&s), 5, 120)); // 02:00 — exclusivo
    assert!(!product_available(Some(&s), 6, 60)); // sábado não herda a janela
}

#[test]
fn produto_24h_e_horarios_invalidos() {
    let vinte_quatro = sched(3, "09:00", "09:00", true); // close == open → 24h
    sweep_product(Some(&vinte_quatro));
    assert!(product_available(Some(&vinte_quatro), 3, 0));
    assert!(product_available(Some(&vinte_quatro), 3, 540));

    for (open, close) in [("", ""), ("9h", "21h"), ("24:00", "12:00"), ("09:00", "12:70")] {
        let s = sched(3, open, close, true);
        sweep_product(Some(&s));
    }
    // `open` inválido → 00:00, `close` inválido → 24:00 → sempre disponível.
    let ambos_ruins = sched(3, "??", "??", true);
    assert!(product_available(Some(&ambos_ruins), 3, 0));
    assert!(product_available(Some(&ambos_ruins), 3, 1439));
}

#[test]
fn produto_semana_completa_e_dias_duplicados() {
    let semana = r#"[
        {"day":0,"open":"00:00","close":"00:00","active":false},
        {"day":1,"open":"08:00","close":"18:00","active":true},
        {"day":2,"open":"18:00","close":"02:00","active":true},
        {"day":3,"open":"00:00","close":"00:00","active":true},
        {"day":6,"open":"10:00","close":"16:00","active":true}
    ]"#;
    sweep_product(Some(semana));
    assert!(product_available(Some(semana), 3, 999)); // dia 24h
    assert!(!product_available(Some(semana), 4, 999)); // dia ausente
    assert!(!product_available(Some(semana), 0, 999)); // dia inativo

    // Dia duplicado: vence a PRIMEIRA entrada nos dois lados.
    let dup = r#"[{"day":1,"open":"08:00","close":"12:00","active":true},
                  {"day":1,"open":"14:00","close":"18:00","active":true}]"#;
    sweep_product(Some(dup));
    assert!(product_available(Some(dup), 1, 600));
    assert!(!product_available(Some(dup), 1, 900));
}

// ---------------------------------------------------------------------------
// Diferença estrutural: o web tem o estado "agora desconhecido" (SSR), o core
// não. Documentada aqui para que a assimetria seja intencional e visível.
// ---------------------------------------------------------------------------

#[test]
fn web_com_agora_desconhecido_nao_tem_equivalente_no_core() {
    // No SSR o web não conhece o relógio → renderiza tudo como
    // disponível/aberto (bom p/ SEO, sem mismatch de hidratação). O core
    // NUNCA fica sem relógio: deriva `(day, mins)` do UTC + offset fixo da
    // empresa, então essa assimetria não afeta a autoridade do checkout.
    let spec: &[DaySpec] = &[(1, "08:00", "18:00", true)];
    assert!(web_av::is_store_open_now(&web_hours(spec), "none", None));
    assert!(web_av::is_available_now(Some(&sched(1, "08:00", "12:00", true)), None));
    // Já o override continua valendo mesmo sem relógio.
    assert!(!web_av::is_store_open_now(&web_hours(spec), "closed", None));
    assert!(web_av::is_store_open_now(&web_hours(spec), "open", None));
    // Mesmo instante nos dois lados (segunda 10:00) → mesma resposta.
    assert!(store_open(spec, "none", 1, 600));
}
