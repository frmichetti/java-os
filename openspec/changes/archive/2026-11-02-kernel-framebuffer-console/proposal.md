# Proposal

## Why

Hoje o kernel só tem um canal de saída: o console serial. Isso é suficiente para desenvolvimento/CI (capturável facilmente pelo QEMU), mas o kernel nunca produziu nenhuma saída visual real numa tela — o primeiro passo concreto do roadmap (`docs/architecture/overview.md`, seção 15, item 6 "HAL e drivers essenciais") e da arquitetura já planejada (seção 5: "Primeiros drivers esperados, em ordem: console serial (já implementado), console de framebuffer, timer/APIC (já implementado), teclado, armazenamento, rede"; "HAL prevista como traits Rust"). O Limine já fornece um framebuffer linear pronto para uso (boot request própria, ainda não consumida pelo kernel) — falta só um driver que desenhe texto nele. Esta change também dá o primeiro passo real da camada de HAL (abstração em traits Rust), preparando o terreno para os próximos drivers (teclado, armazenamento, rede) seguirem o mesmo padrão em vez de cada um inventar sua própria interface.

## What Changes

- Nova abstração de HAL: um trait `Console` mínimo (escrever texto, limpar a tela) que desacopla "emitir uma linha de diagnóstico" do dispositivo físico que a renderiza.
- Novo driver de console de framebuffer: desenha texto no framebuffer linear que o Limine já fornece (boot request nova, `FramebufferRequest`, ainda não declarada em `kernel/src/main.rs`), com uma fonte bitmap embutida no próprio binário do kernel (sem carregar nada de disco — não há sistema de arquivos ainda) e rolagem de linhas quando a tela enche.
- O console serial **continua existindo e sendo o canal já validado pelo smoke test de boot** — o framebuffer é um canal adicional, não um substituto. A partir do momento em que o framebuffer estiver pronto (depois de HHDM — ver design.md), as mensagens de diagnóstico do kernel passam a aparecer nos dois canais.
- **Sem mudança em nenhum contrato já especificado**: `kernel-boot` já exige o banner/mapa de memória "no console serial" especificamente — isso continua valendo exatamente como está; o framebuffer é observável a mais, não uma substituição.

## Capabilities

### New Capabilities
- `kernel-hal`: a primeira abstração da camada de HAL — um trait `Console` para saída de texto, independente do dispositivo físico que a implementa.
- `kernel-framebuffer-console`: um console de texto renderizado no framebuffer linear fornecido pelo Limine, implementando `Console` — fonte bitmap embutida, rolagem de linhas.

### Modified Capabilities
- Nenhuma — `kernel-boot` continua exigindo o banner/mapa de memória especificamente no console serial, já satisfeito hoje; o framebuffer é uma capability nova e aditiva, não uma mudança de contrato existente.

## Impact

- **Novos módulos no crate `kernel`**: `kernel::hal` (trait `Console`) e `kernel::framebuffer` (driver de console de framebuffer, incluindo a fonte bitmap embutida).
- **`kernel/src/main.rs`**: nova `FramebufferRequest` (crate `limine`, já na versão 0.6.5 usada pelo projeto); inicialização do console de framebuffer em `continue_boot`, depois de HHDM pronto.
- **Mensagens de diagnóstico existentes** (banner, mapa de memória, heap, APIC, timer, escalonador, syscall) passam a também aparecer no framebuffer a partir do ponto em que ele é inicializado, sem alterar o que já é escrito no console serial.
- **Fora de escopo nesta change** (ver design.md para o racional completo): teclado, armazenamento, rede (próximos drivers da mesma seção do roadmap, cada um uma change futura); gráficos além de texto (sem formas, imagens ou múltiplas janelas); qualquer leitura de entrada do usuário.
- **Nenhuma dependência externa nova esperada** além da já existente crate `limine` (só uma nova boot request dela).
