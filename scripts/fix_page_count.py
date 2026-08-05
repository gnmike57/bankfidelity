"""Restore the pdfium_render page_count implementation in runtime.rs."""

with open('src/app/runtime.rs', 'r', encoding='utf-8') as f:
    content = f.read()

target = '''                            // Stage 3 / Item #16: page count first
                            let page_count = {
                                // Phase 0: stub page count (awaiting oxidize-pdf integration)
                                1
                            };'''
replacement = '''                            // Stage 3 / Item #16: page count first
                            let page_count = {
                                let p = input.clone();
                                tokio::task::spawn_blocking(move || -> usize {
                                    use pdfium_render::prelude::Pdfium;
                                    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
                                        .or_else(|_| Pdfium::bind_to_system_library());
                                    let pdfium = match bindings {
                                        Ok(b) => Pdfium::new(b),
                                        Err(e) => {
                                            tracing::error!("Failed to bind Pdfium: {}", e);
                                            return 0;
                                        }
                                    };
                                    pdfium
                                        .load_pdf_from_file(&p, None)
                                        .map(|d| d.pages().len() as usize)
                                        .unwrap_or(0)
                                })
                                .await
                                .unwrap_or(0)
                            };'''

if target in content:
    content = content.replace(target, replacement)
    with open('src/app/runtime.rs', 'w', encoding='utf-8') as f:
        f.write(content)
    print('Restored pdfium_render page_count')
else:
    print('Target not found')
