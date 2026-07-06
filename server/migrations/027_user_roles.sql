-- Adiciona o nível de acesso (role) ao usuário operador.
--
-- Regras aplicadas (AI_RULES.md §11):
-- - 'super_admin': escopo cross-tenant (Fase 2 — endpoints próprios)
-- - 'admin': dono do estabelecimento
-- - 'employee': colaborador
--
-- Default 'admin' garante que usuários pré-existentes (donos cadastrados
-- antes desta migração) mantenham acesso operacional total.

ALTER TABLE users ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'admin';

-- Constraint impedindo valores fora dos 3 níveis suportados.
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_role_check;
ALTER TABLE users ADD CONSTRAINT users_role_check
    CHECK (role IN ('super_admin', 'admin', 'employee'));

CREATE INDEX IF NOT EXISTS idx_users_role ON users(role);
