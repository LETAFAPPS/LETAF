-- Modo de codificação do EAN-13 emitido por balanças (somente para unit=kg).
--
-- Regras aplicadas (AI_RULES.md §11):
-- - 'weight': peso em gramas | 'price': preço em centavos
-- - Validação delegada ao service (`BalanceMode::from_db_str`) — SQLite
--   não suporta ALTER ADD CHECK em tabela existente.

ALTER TABLE products ADD COLUMN balance_mode TEXT NOT NULL DEFAULT 'weight';
