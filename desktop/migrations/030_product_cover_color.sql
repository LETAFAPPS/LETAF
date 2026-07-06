-- Cor de fundo detectada nas bordas da imagem do produto (heurística no
-- upload). `NULL` = imagem transparente ou indetectável → fallback do tema.
ALTER TABLE products
    ADD COLUMN cover_color TEXT;
