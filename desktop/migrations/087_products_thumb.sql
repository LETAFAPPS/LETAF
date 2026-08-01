-- Miniatura da imagem do produto (~64px, base64).
--
-- A lista desenha um quadradinho por linha, mas carregava a imagem de
-- DETALHE (PNG 400×400, 77 a 106 KB) e decodificava cada uma. Com 3.334
-- produtos isso dava 301 MB e 482 ms só para abrir a tela. A miniatura pesa
-- ~2 KB — 31× menos — e a listagem deixa de ler `image_data`.
--
-- Coluna DERIVADA de `image_data`: fica NULL nos produtos já cadastrados e é
-- preenchida em segundo plano no boot do desktop (ver `thumbs::backfill`).
ALTER TABLE products ADD COLUMN thumb_data TEXT;
