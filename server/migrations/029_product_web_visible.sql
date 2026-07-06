-- Adiciona controle de visibilidade do produto no cardápio web.
--
-- Regras aplicadas (AI_RULES.md §11):
-- - `active` = ativo global (cardápio + PDV). Quando false, o produto não
--   aparece em lugar nenhum (desativado).
-- - `web_visible` = visibilidade somente no cardápio web. Quando false e
--   `active` true, o produto continua aparecendo no PDV mas é oculto na web.
-- - Default true mantém compatibilidade com produtos pré-existentes.

ALTER TABLE products ADD COLUMN IF NOT EXISTS web_visible BOOLEAN NOT NULL DEFAULT TRUE;
