# Tasks

## 1. `kernel::hal`: trait `Console`

- [x] 1.1 Criar `kernel/src/hal.rs` com `pub trait Console: core::fmt::Write {}`; registrar o módulo em `kernel/src/lib.rs`. Verificar que `cargo build` compila.

## 2. `kernel::framebuffer`: estruturas puras (pixel, fonte, posição do cursor)

- [x] 2.1 Declarar `FRAMEBUFFER_REQUEST: FramebufferRequest` em `kernel/src/main.rs` (mesmo padrão de `MEMORY_MAP_REQUEST`/`HHDM_REQUEST`). Verificar que `cargo build` compila (a resposta ainda não é consumida nesta tarefa).
- [x] 2.2 Implementar a conversão de cor (r, g, b de 8 bits) para o valor de pixel, genérica a partir de `red/green/blue_mask_size`/`_shift` (ver design.md). Extrair como função pura e verificar com teste unitário em host que, para máscaras de 8 bits em posições conhecidas (ex.: RGB888 em 32 bpp), o valor produzido bate com o esperado, e que truncar um canal para uma máscara menor (ex.: 5 bits) descarta os bits menos significativos corretamente.
- [x] 2.3 Definir a tabela de fonte bitmap 8×8 (autoral, ASCII 32-126, ver design.md) e uma função `glyph_for(c: char) -> &'static [u8; 8]` com um glifo de reserva para qualquer caractere fora do intervalo coberto. Verificar com teste unitário em host que caracteres dentro do intervalo devolvem um glifo não vazio (pelo menos um bit ligado, exceto o espaço) e que um caractere fora do intervalo (ex.: um caractere acentuado) devolve o glifo de reserva, nunca um acesso fora dos limites da tabela.
- [x] 2.4 Implementar o rastreamento de posição do cursor lógico (linha/coluna atual, dado o tamanho do console em caracteres) como lógica pura: avançar uma coluna, quebrar linha ao fim da largura, e sinalizar quando uma rolagem é necessária ao ultrapassar a última linha. Verificar com testes unitários em host cobrindo: avanço normal de coluna, quebra de linha, e o sinal de rolagem disparando exatamente ao escrever além da última linha (nunca antes, nunca depois).

## 3. `kernel::framebuffer`: driver real (desenho de pixels, rolagem, `Console`)

- [x] 3.1 Implementar `FramebufferConsole` (guarda o ponteiro base, `width`/`height`/`pitch`/`bpp`/máscaras do `Framebuffer` reportado, e a posição do cursor da tarefa 2.4): desenhar um glifo (tarefa 2.3) nos pixels correspondentes usando a conversão de cor (tarefa 2.2), e implementar `core::fmt::Write`/`Console` escrevendo caractere a caractere, avançando o cursor. Como desenhar pixels de verdade depende de memória de framebuffer real (não disponível em host), verificar que `cargo build`/`cargo kernel-build` compilam; o desenho real é verificado visualmente em boot na tarefa 5.1.
- [x] 3.2 Implementar a rolagem (copiar as linhas de pixels de baixo para cima, limpar a última linha com a cor de fundo) quando o sinal da tarefa 2.4 disparar. Verificar que `cargo build`/`cargo kernel-build` compilam; comportamento real verificado visualmente na tarefa 5.1.
- [x] 3.3 Implementar a inicialização (`kernel::framebuffer::init(response: &FramebufferResponse) -> Result<(), FramebufferError>` ou equivalente): valida `memory_model == FRAMEBUFFER_RGB` e `bpp` em `{16, 24, 32}` (ver design.md — Non-Goals), rejeitando explicitamente qualquer outra coisa como `FramebufferError` em vez de desenhar pixels com suposições erradas. Verificar com teste unitário em host a lógica pura de validação (quais combinações de `memory_model`/`bpp` são aceitas/rejeitadas), sem tocar memória real de framebuffer.

## 4. Integração: espelhamento e inicialização em `kernel_main`

- [x] 4.1 Implementar `kernel::framebuffer::mirror(args: core::fmt::Arguments)` (escreve no console de framebuffer global, se já inicializado; não faz nada caso contrário) e chamá-la a partir de `kernel::serial::_print` (ver design.md — "Espelhamento"), depois da escrita no console serial. Verificar que `cargo build` compila e que `cargo test -p kernel --lib` continua passando sem regressão.
- [x] 4.2 Em `kernel_main`, logo após `serial::init()` (antes de `HHDM_REQUEST`, ver design.md — "Inicialização cedo"): obter a resposta de `FRAMEBUFFER_REQUEST`, inicializar o console de framebuffer com o primeiro framebuffer reportado (tarefa 3.3), e tratar explicitamente a ausência/indisponibilidade (nenhuma resposta, nenhum framebuffer, ou um framebuffer rejeitado pela validação da tarefa 3.3) reportando pelo console serial e prosseguindo o boot normalmente — cobre "Indisponibilidade do Framebuffer é Tratada Explicitamente" de `specs/kernel-framebuffer-console/spec.md`. Verificar com `cargo xtask test` que o boot continua passando sem regressão no console serial.

## 5. Verificação end-to-end e documentação

- [x] 5.1 Validar visualmente, rodando `cargo xtask run` (com display, sem `-display none`), que o banner de boot e as mensagens de diagnóstico subsequentes aparecem corretamente desenhadas na tela, incluindo pelo menos uma rolagem real (o mapa de memória tem linhas suficientes para isso). **Se a hipótese de `Framebuffer::address()` já vir mapeado (design.md) se provar errada** (tela não mostra nada, ou falha), ajustar a inicialização para converter o endereço como físico via `hhdm::phys_to_virt` depois de `hhdm::init()`, e repetir esta verificação — cobre os requisitos de `specs/kernel-framebuffer-console/spec.md`.
- [x] 5.2 Confirmar que `cargo xtask test` (smoke test automatizado, baseado só no console serial) continua passando sem nenhuma mudança de comportamento esperado — cobre que esta change não altera o contrato já especificado de `kernel-boot`.
- [x] 5.3 Atualizar `docs/architecture/overview.md`: seção "5. HAL e Drivers", descrevendo a primeira abstração de HAL (`trait Console`) e o console de framebuffer implementados, e o roadmap completo (observação no item 6, já que ele cobre múltiplos drivers — esta change entrega só a primeira parte: HAL + framebuffer; teclado/armazenamento/rede continuam pendentes). Verificar lendo o arquivo atualizado.

## Workflow follow-up

- Arquivar esta change após a aprovação do usuário para as modificações propostas.
