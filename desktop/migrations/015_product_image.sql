-- Adiciona campo de imagem ao produto (AI_RULES.md §6).
-- TEXT armazena o base64 da imagem; NULL indica produto sem imagem.
ALTER TABLE products ADD COLUMN image_data TEXT;
