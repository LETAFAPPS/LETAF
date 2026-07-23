-- Foto de perfil do operador (espelha a coluna do servidor para sync).
-- Imagem JPEG/PNG em base64; NULL = sem foto.
ALTER TABLE users ADD COLUMN avatar TEXT;
