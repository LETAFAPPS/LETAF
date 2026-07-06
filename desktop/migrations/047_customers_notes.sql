-- Espelha server/049_customers_notes.sql (Fase 9).
-- Observação interna do estabelecimento sobre o cliente.
ALTER TABLE customers ADD COLUMN notes TEXT NULL;
