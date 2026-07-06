-- Adiciona o nível de acesso (role) ao usuário operador.
--
-- Regras aplicadas (AI_RULES.md §11):
-- - 'admin': dono do estabelecimento (default para usuários pré-existentes)
-- - 'employee': colaborador
-- - 'super_admin': uso futuro (Fase 2)
--
-- SQLite aceita CHECK constraint inline mas não suporta ALTER ADD CONSTRAINT —
-- a validação fica delegada ao service `UserRole::from_db_str` no caller.

ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'admin';

CREATE INDEX IF NOT EXISTS idx_users_role ON users(role);
