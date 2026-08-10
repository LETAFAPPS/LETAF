-- Galeria de imagens ADICIONAIS por produto (recurso da "loja"). A imagem
-- principal continua em products.image_data; estas são extras (carrossel no
-- cardápio). Espelha product_ingredients: regravada inteira no upsert do
-- produto (viaja junto no sync). Ordem = sort_order.
CREATE TABLE IF NOT EXISTS product_images (
    company_id  UUID NOT NULL REFERENCES companies(id),
    product_id  UUID NOT NULL REFERENCES products(id),
    sort_order  INTEGER NOT NULL DEFAULT 0,
    image_data  TEXT NOT NULL,
    PRIMARY KEY (company_id, product_id, sort_order)
);
CREATE INDEX IF NOT EXISTS idx_product_images_product ON product_images(company_id, product_id);
