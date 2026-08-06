-- Localização do estabelecimento agora é um LINK (URL do mapa), não mais
-- coordenadas latitude/longitude. Substitui a migration 074_company_geo.
ALTER TABLE companies ADD COLUMN location_url TEXT;
ALTER TABLE companies DROP COLUMN latitude;
ALTER TABLE companies DROP COLUMN longitude;
