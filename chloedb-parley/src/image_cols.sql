,
      caption TEXT OPTIONS(derive = 'image', model = 'blip-image-captioning-base'),
      img_emb VECTOR OPTIONS(embed = 'image', model = 'siglip-base-patch16-224')