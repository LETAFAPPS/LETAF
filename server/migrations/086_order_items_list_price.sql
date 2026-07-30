-- Snapshot do preço unitário de tabela (produto + adicionais, sem o
-- desconto do produto) por item — permite exibir o desconto no recibo/
-- detalhe depois da venda. NULL = pedido antigo ou sem informação.
-- Espelha desktop/078_order_items_list_price.sql.
ALTER TABLE order_items ADD COLUMN list_unit_price NUMERIC(14,2);
