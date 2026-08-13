-- Tema padrão do site (claro/escuro) escolhido pela empresa: 'light' |
-- 'dark' | NULL (automático). NÃO sobrepõe a escolha do visitante. Config
-- da empresa; sincroniza como os demais campos de Configurações.
ALTER TABLE companies ADD COLUMN default_scheme TEXT;
