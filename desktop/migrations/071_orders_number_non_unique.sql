-- Troca o índice UNIQUE de `number` por NÃO-único no desktop.
--
-- Motivo (§7): o servidor é a AUTORIDADE do número do pedido e renumera no
-- conflito de sync (colisão offline×web). Ao puxar, o desktop pode receber
-- transitoriamente dois pedidos com o mesmo número (o pedido web com o número
-- liberado + o pedido local ainda com o número antigo) ANTES de o pedido local
-- reconverter para o número renumerado. Com o índice UNIQUE, esse INSERT
-- transitório violava a constraint e ABORTAVA o pull inteiro de pedidos
-- (cursor congelado → sync travado para sempre). Sem a unicidade local, o
-- INSERT passa e o `ON CONFLICT DO UPDATE SET number = excluded.number`
-- reconverge no mesmo ciclo. A unicidade real fica garantida no servidor
-- (Postgres mantém o UNIQUE + renumeração); a criação local é serializada
-- (processo único), então não gera duplicata de fato.
DROP INDEX IF EXISTS idx_orders_company_number_unique;
CREATE INDEX IF NOT EXISTS idx_orders_company_number ON orders(company_id, number);
