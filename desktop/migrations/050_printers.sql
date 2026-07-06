-- Tabela de impressoras cadastradas localmente no desktop.
--
-- IMPORTANTE: esta tabela é **per-device**. Diferente das demais
-- entidades do projeto, ela NÃO sincroniza com o servidor — cada
-- desktop tem suas próprias impressoras físicas. O campo `synced`
-- é mantido por consistência de schema com a BaseFields do core,
-- mas vai SEMPRE com valor 1 (true) para o SyncWorker pular o
-- registro nas queries `find_unsynced`.
--
-- Não há equivalente em `server/` por isso.

CREATE TABLE printers (
    id           TEXT      PRIMARY KEY,
    company_id   TEXT      NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name         TEXT      NOT NULL,                     -- rótulo livre ("Cozinha 1")
    kind         TEXT      NOT NULL,                     -- 'order' | 'kitchen' | 'fiscal'
    system_name  TEXT      NOT NULL,                     -- nome no SO ("EPSON TM-T20")
    is_default   BOOLEAN   NOT NULL DEFAULT 0,           -- padrão para o `kind`
    paper_width  INTEGER   NOT NULL DEFAULT 80,          -- 58 ou 80 (mm)
    created_at   TEXT      NOT NULL,
    updated_at   TEXT      NOT NULL,
    deleted_at   TEXT      NULL,
    synced       BOOLEAN   NOT NULL DEFAULT 1            -- sempre 1: não sincroniza
);

-- Lookup mais frequente: "qual a impressora padrão de cozinha desta
-- empresa?" — acontece a cada impressão de comanda/ticket.
CREATE INDEX printers_company_kind_default_idx
    ON printers (company_id, kind, is_default)
    WHERE deleted_at IS NULL;
