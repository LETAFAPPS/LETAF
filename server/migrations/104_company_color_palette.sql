-- Paleta de cores do site (cardápio web) escolhida por CADA empresa nas
-- Configurações. Slug de um conjunto fixo (core::theme_palette). NULL = usa o
-- tema do tipo. Config da empresa (editável por ela; sincroniza como logo).
ALTER TABLE companies
    ADD COLUMN IF NOT EXISTS color_palette TEXT;
