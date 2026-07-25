-- Funções de administrador (RBAC do painel do super admin). Global.
CREATE TABLE IF NOT EXISTS admin_roles (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    screens     TEXT NOT NULL DEFAULT '',   -- CSV das chaves de tela liberadas
    created_at  TIMESTAMP NOT NULL,
    updated_at  TIMESTAMP NOT NULL,
    deleted_at  TIMESTAMP
);

-- Atribuição usuário → função (1:1). Sem linha = master (acesso total).
CREATE TABLE IF NOT EXISTS admin_user_roles (
    user_id  UUID PRIMARY KEY,
    role_id  UUID NOT NULL
);
