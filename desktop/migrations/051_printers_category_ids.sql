-- Roteamento de itens por categoria.
--
-- Cada impressora ganha uma lista de IDs de categorias que ela atende
-- (armazenada como JSON em coluna TEXT). Quando vazio (`'[]'`), a
-- impressora age como "catch-all": recebe todos os itens da cozinha.
-- Quando preenchido, só recebe itens de produtos dessas categorias.
--
-- Afeta apenas comandas com `kind = 'kitchen'`. A comanda do cliente
-- (`kind = 'order'`) ignora esse campo — sempre imprime tudo num
-- único papel.

ALTER TABLE printers ADD COLUMN category_ids TEXT NOT NULL DEFAULT '[]';
