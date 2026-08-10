-- Paleta de cores do site escolhida pela empresa (slug de core::theme_palette).
-- NULL = usa o tema do tipo. Config da empresa; sincroniza como os demais
-- campos de Configurações.
ALTER TABLE companies ADD COLUMN color_palette TEXT;
