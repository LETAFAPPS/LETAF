-- Forma de pagamento dos pedidos (preenchido somente em vendas PDV).
-- Espelha desktop/migrations/052_orders_payment_method.sql.

ALTER TABLE orders ADD COLUMN payment_method TEXT NULL;
