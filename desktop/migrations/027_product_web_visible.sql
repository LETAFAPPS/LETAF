-- Adiciona controle de visibilidade do produto no cardápio web.
--
-- Regras aplicadas (AI_RULES.md §11):
-- - `active` = ativo global; quando false, oculta em todos lugares.
-- - `web_visible` = visibilidade no cardápio web; quando false e active=true,
--   o produto continua disponível no PDV mas oculto na web.

ALTER TABLE products ADD COLUMN web_visible INTEGER NOT NULL DEFAULT 1;
