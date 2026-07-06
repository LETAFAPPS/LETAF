-- Remove a tabela suppliers que foi criada na migration 003 mas nunca utilizada
-- (nenhum model, service ou repository foi implementado para esta entidade).
-- Violava AI_RULES.md §8 (evitar código morto) e §14.

DROP TABLE IF EXISTS suppliers;
