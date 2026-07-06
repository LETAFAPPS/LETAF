-- Forma de pagamento dos pedidos (preenchido somente em vendas PDV).
--
-- `NULL` = pedido sem forma cadastrada (pedidos vindos do cardápio
-- web). Valores aceitos: 'cash' | 'credit' | 'debit' | 'pix' | 'other'.
-- Validação fica no service (`order::model::PAYMENT_METHODS`) — schema
-- aceita qualquer texto para tolerar adição futura sem migration.
--
-- Espelha server/052_orders_payment_method.sql.

ALTER TABLE orders ADD COLUMN payment_method TEXT NULL;
