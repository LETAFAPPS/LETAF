-- Coordenadas geográficas do estabelecimento (mapa/entrega).
ALTER TABLE companies ADD COLUMN latitude REAL;
ALTER TABLE companies ADD COLUMN longitude REAL;
