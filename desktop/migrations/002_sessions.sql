-- Tabela de sessão local — SQLite (desktop)
--
-- Regras aplicadas (AI_RULES.md §5, §10):
-- - Desktop usa SQLite
-- - Acesso ao banco somente via repository (SessionStore)
--
-- Armazena token JWT e company_id para persistir sessão entre reinícios.
-- Não é entidade de domínio — é infraestrutura do desktop.

CREATE TABLE IF NOT EXISTS sessions (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
