-- Cor de fundo detectada nas bordas da imagem do produto.
-- `NULL` quando a imagem é transparente ou a heurística não conseguiu
-- detectar uma cor sólida uniforme nos cantos.
ALTER TABLE products
    ADD COLUMN IF NOT EXISTS cover_color TEXT;
