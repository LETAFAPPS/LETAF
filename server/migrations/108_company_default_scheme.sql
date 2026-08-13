-- Tema padrão do site (cardápio web) com que o cardápio INICIA para o
-- visitante: 'light' | 'dark' | NULL (automático, segue o sistema). NÃO
-- sobrepõe a escolha manual do visitante (localStorage). Config da empresa
-- (editável por ela; sincroniza como color_palette).
ALTER TABLE companies
    ADD COLUMN IF NOT EXISTS default_scheme TEXT;
