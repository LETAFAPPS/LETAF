-- Separa a VERSÃO DO CADASTRO do marcador de mudança do produto.
--
-- `products.updated_at` acumulava dois papéis incompatíveis:
--   (a) versão para o LWW do upsert (`EXCLUDED.updated_at > products.updated_at`);
--   (b) marcador de mudança para o cursor de pull dos outros terminais.
--
-- `apply_stock_movement` precisa de (b) — sem bumpar `updated_at` ao aplicar
-- um delta, a venda de um terminal nunca chegava aos outros. Mas o bump usa
-- `now()` do servidor, então ele também vence (a): o push do PRODUTO chega
-- logo depois com o `updated_at` do momento da edição (sempre anterior ao
-- `now()`) e cai fora do `WHERE`.
--
-- Efeito reproduzido: o operador edita nome/preço e mexe no estoque no MESMO
-- salvamento. No ciclo seguinte o movimento sobe primeiro e joga
-- `products.updated_at` para `now()`; o produto sobe em seguida e é
-- DESCARTADO (UPDATE 0). O desktop marca `synced=true` assim mesmo (a rota
-- responde 2xx), e o pull seguinte traz a versão antiga do servidor,
-- revertendo a edição também no local. A alteração some sem erro nenhum.
--
-- `content_updated_at` passa a ser a versão do CADASTRO, tocada só pelo
-- upsert. `updated_at` fica sendo só o marcador de mudança, e nunca anda
-- para trás.

ALTER TABLE products
    ADD COLUMN IF NOT EXISTS content_updated_at timestamp;

UPDATE products SET content_updated_at = updated_at
 WHERE content_updated_at IS NULL;

ALTER TABLE products
    ALTER COLUMN content_updated_at SET NOT NULL;
