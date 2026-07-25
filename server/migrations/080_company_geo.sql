-- Coordenadas geográficas do estabelecimento (mapa/entrega).
ALTER TABLE companies ADD COLUMN latitude DOUBLE PRECISION;
ALTER TABLE companies ADD COLUMN longitude DOUBLE PRECISION;
