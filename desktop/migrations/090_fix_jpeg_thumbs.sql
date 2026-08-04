-- Invalida miniaturas gravadas em JPEG cuja imagem-fonte NÃO é JPEG
-- (PNG/WebP podem ter transparência). Um thumb JPEG de origem transparente
-- carrega um fundo sólido "queimado" (a faixa/sombra reportada) que nenhum
-- truque de fundo no Slint remove — os pixels do fundo estão baked-in.
--
-- Ao zerar, o backfill de boot (find_sem_miniatura → make_thumbnail) regenera
-- a miniatura no formato correto (PNG quando há alpha real). Miniaturas JPEG
-- de origem também JPEG (opacas) são mantidas — estão corretas.
--
-- base64 do cabeçalho JPEG (0xFFD8FF) começa com "/9j/"; do PNG (0x89504E47)
-- com "iVBOR". A comparação por prefixo evita decodificar o base64.
UPDATE products
SET thumb_data = NULL
WHERE thumb_data LIKE '/9j/%'
  AND image_data IS NOT NULL
  AND image_data <> ''
  AND image_data NOT LIKE '/9j/%';
