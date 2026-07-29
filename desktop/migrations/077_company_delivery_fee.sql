-- Taxa de entrega (frete) por empresa. Aplicada automaticamente em
-- pedidos do tipo Delivery no PDV. Dinheiro no SQLite = REAL (§13).
-- Espelha server/085_company_delivery_fee.sql.
ALTER TABLE companies ADD COLUMN delivery_fee REAL NOT NULL DEFAULT 0;
