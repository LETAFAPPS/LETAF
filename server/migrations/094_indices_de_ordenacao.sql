-- Espelho da migration 086 do SQLite: índices que casam com o FILTRO e a
-- ORDENAÇÃO das consultas de listagem.
--
-- Vale para o servidor pelos mesmos motivos, e com um agravante: aqui as
-- tabelas guardam TODAS as empresas, então uma ordenação sem índice
-- adequado percorre e ordena linhas de todos os tenants.
--
-- A ordem das colunas segue cada consulta: igualdades primeiro, ordenação
-- depois. `DESC` no índice para casar com o `ORDER BY ... DESC` — o Postgres
-- também consegue varrer um índice ASC ao contrário, mas deixar explícito
-- evita depender da escolha do planejador.

CREATE INDEX IF NOT EXISTS idx_wallet_movements_extrato
    ON wallet_movements(company_id, account_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_treasury_movements_extrato
    ON treasury_movements(company_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_cash_movements_sessao
    ON cash_movements(company_id, session_id, created_at);

CREATE INDEX IF NOT EXISTS idx_products_company_created
    ON products(company_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_customers_company_created
    ON customers(company_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_order_items_pedido
    ON order_items(company_id, order_id, created_at);

-- Estatísticas para o planejador. O autovacuum roda `ANALYZE` sozinho, mas
-- só depois de acumular alterações — logo após criar índice, uma passada
-- explícita evita a janela em que o planejador ainda os ignora.
ANALYZE wallet_movements;
ANALYZE treasury_movements;
ANALYZE cash_movements;
ANALYZE products;
ANALYZE customers;
ANALYZE order_items;
