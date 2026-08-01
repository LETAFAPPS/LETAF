-- Índices que casam com o FILTRO e a ORDENAÇÃO das consultas de tela.
--
-- Medido num banco inflado para volume realista (30 mil pedidos, 60 mil
-- movimentos, 5 mil produtos): sem estes índices o SQLite escolhia um índice
-- que só resolvia o `company_id` e depois fazia `USE TEMP B-TREE FOR ORDER
-- BY` — materializava e ordenava o conjunto INTEIRO em memória a cada
-- abertura de tela.
--
-- O ganho aparece onde há `LIMIT`: a consulta passa a parar nas primeiras N
-- linhas em vez de ordenar tudo para descartar o resto. Nos movimentos de
-- carteira e tesouraria (que já usam `LIMIT`) foi de 10,7 ms para 0,07 ms —
-- 150×. Em consultas que devolvem TUDO o índice não muda nada, porque o
-- custo é o próprio volume; essas dependem de paginação, não de índice.
--
-- A ordem das colunas segue exatamente cada consulta: primeiro as de
-- igualdade, depois a de ordenação.

-- `find_movements(company_id, account_id, limit)` — extrato da carteira.
CREATE INDEX IF NOT EXISTS idx_wallet_movements_extrato
    ON wallet_movements(company_id, account_id, created_at DESC);

-- `find_movements(company_id, limit)` — ledger da tesouraria.
CREATE INDEX IF NOT EXISTS idx_treasury_movements_extrato
    ON treasury_movements(company_id, created_at DESC);

-- Movimentos de uma sessão de caixa (ordem ASC — o resumo lê em ordem).
CREATE INDEX IF NOT EXISTS idx_cash_movements_sessao
    ON cash_movements(company_id, session_id, created_at);

-- Listas ordenadas por data de criação.
CREATE INDEX IF NOT EXISTS idx_products_company_created
    ON products(company_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_customers_company_created
    ON customers(company_id, created_at DESC);

-- Itens de um pedido (o detalhe abre um pedido por vez).
CREATE INDEX IF NOT EXISTS idx_order_items_pedido
    ON order_items(company_id, order_id, created_at);

-- Estatísticas para o planejador escolher os índices acima. Sem `ANALYZE` o
-- SQLite decide por heurística de forma e pode preferir um índice pior.
ANALYZE;
