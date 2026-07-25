-- Ativa/desativa o acesso de um admin restrito (o master não tem linha e é
-- sempre ativo). Ausência de linha = ativo.
ALTER TABLE admin_user_roles ADD COLUMN IF NOT EXISTS active BOOLEAN NOT NULL DEFAULT TRUE;
