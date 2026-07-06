-- Adiciona slug do ícone à categoria (Fase 6 - Categorias com ícone).
-- O slug aponta para um SVG embarcado nos clientes (web/desktop).
-- Categorias existentes ficam com NULL = sem ícone (UI mostra
-- placeholder neutro), sem migração de dados necessária.

ALTER TABLE categories ADD COLUMN icon_name TEXT NULL;
