# Qwen-Image en WSL (evaluación local)

Este perfil es independiente del chat Ollama privado administrado por Seiya.
El generador usa CPU y cuatro hilos. Los pesos del generador y del codificador,
junto con el VAE, suman 9,34 GiB en disco. No arrancar varias generaciones.

Ejemplo experimental sin validación de generación extremo a extremo. Copiar
`qwen_endpoint.py` y `catalog-qwen.json` a `~/.local/share/seiya-image/`. El
adaptador espera `runtime/build/bin/sd-cli` y los tres pesos bajo
`qwen-image-2.1-turbo-q4/` en ese directorio. Usar las revisiones, nombres
locales y hashes del [manifiesto](../../contracts/benchmarks/image-candidates/v0.1.0/qwen-image21-turbo-q4.json).

1. En una terminal, ejecutar el adaptador:

   ```sh
   python3 ~/.local/share/seiya-image/qwen_endpoint.py
   ```

2. En otra terminal, iniciar un motor dedicado de texto con el Qwen ya instalado:

   ```sh
   OLLAMA_HOST=127.0.0.1:21011 OLLAMA_NO_CLOUD=1 OLLAMA_NOPRUNE=1 \
     OLLAMA_MODELS="$HOME/.local/state/Seiya/ollama-models" ollama serve
   ```

3. Una vez detenida la instancia anterior de Seiya (puerto 8015), abrir el perfil:

   ```sh
   seiya-rust --launch --catalog "$HOME/.local/share/seiya-image/catalog-qwen.json"
   ```

   Revisar que el Cerebro sea `brain-tools`, el pipeline `image-local` y los
   campos de ejecutable/assets de imagen estén vacíos. El catálogo no reemplaza
   `local-models.json`; confirmar el perfil sí guarda las asignaciones de roles.
   El chat debe producir una llamada `image_generate`, no solo texto. La
   compatibilidad con tools depende del modelo de texto y de su plantilla.

Para cambiar de generador, registrar en `image_models` otro ID, binding
exacto y URL de su propio motor compatible. Seleccionarlo en `image` o desde
el selector de pipeline. Los parámetros del CLI de este adaptador son
específicos de Qwen-Image 2.1 Turbo; no apuntarlo a pesos de otra arquitectura.
Detener los dos motores externos con Ctrl+C en sus respectivas terminales.
