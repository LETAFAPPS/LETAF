-- Taxa de entrega (frete) por empresa. Aplicada automaticamente em
-- pedidos do tipo Delivery no PDV. Dinheiro no PostgreSQL = NUMERIC.
-- Espelha desktop/077_company_delivery_fee.sql.
ALTER TABLE companies ADD COLUMN delivery_fee NUMERIC(14,2) NOT NULL DEFAULT 0;
