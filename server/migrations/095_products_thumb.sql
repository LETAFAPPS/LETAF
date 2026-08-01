-- Espelho da 087 do SQLite: miniatura da imagem do produto.
--
-- Precisa existir no servidor para viajar no sync — regerar a miniatura em
-- cada terminal custaria decodificar e reescalar a imagem inteira; sincronizar
-- 2 KB é mais barato que isso.
ALTER TABLE products ADD COLUMN IF NOT EXISTS thumb_data TEXT;
