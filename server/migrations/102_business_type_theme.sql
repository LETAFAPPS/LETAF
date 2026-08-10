-- Tema visual do site (cardápio) por tipo de empresa. Slug de um conjunto
-- fixo (restaurante|loja|fabrica); o `web` tem um preset de CSS por slug.
-- `restaurante` = visual atual do sistema (default).
ALTER TABLE business_types
    ADD COLUMN IF NOT EXISTS theme TEXT NOT NULL DEFAULT 'restaurante';

-- Semeia os 3 tipos "de fábrica" já preparados, com UUIDs fixos e
-- idempotência por id (não duplica se já existirem / em re-run). O super
-- admin pode editá-los/removê-los depois pelo painel.
INSERT INTO business_types (id, name, description, theme, active, sort_order, created_at, updated_at)
VALUES
    ('a1b2c3d4-0001-4000-8000-000000000001', 'Restaurante', 'Cardápio com adicionais, variações e ficha técnica.', 'restaurante', TRUE, 1, NOW(), NOW()),
    ('a1b2c3d4-0002-4000-8000-000000000002', 'Loja',        'Varejo: catálogo de produtos, estoque e código de barras.', 'loja', TRUE, 2, NOW(), NOW()),
    ('a1b2c3d4-0003-4000-8000-000000000003', 'Fábrica',     'Produção: ficha técnica (insumos) e controle de estoque.', 'fabrica', TRUE, 3, NOW(), NOW())
ON CONFLICT (id) DO NOTHING;
