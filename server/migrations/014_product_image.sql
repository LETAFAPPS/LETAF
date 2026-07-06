-- Adiciona campo de imagem ao produto no PostgreSQL (AI_RULES.md §6).
-- TEXT armazena o base64 da imagem; NULL indica produto sem imagem.
ALTER TABLE products ADD COLUMN IF NOT EXISTS image_data TEXT;
