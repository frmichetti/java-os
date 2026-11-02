# Proposal

## Why

O kernel hoje roda inteiramente sobre a stack que o Limine configura antes de entregar o controle — não há nenhuma stack própria do kernel, nem uma guard page (página deliberadamente não mapeada) logo abaixo dela. Isso foi descoberto como um know issue (`KNOW_ISSUES.md`) durante a verificação de double fault em `kernel-interrupts-timer`: um estouro real de stack não falha rapidamente como esperado — em vez disso, sobrescreve silenciosamente uma quantidade significativa de memória adjacente (incluindo potencialmente estruturas do próprio kernel) antes de eventualmente alcançar algo que gera falha, e mesmo quando a falha ocorre, o diagnóstico pode não ser reportado de forma confiável devido à extensão da corrupção já causada. Com a paginação própria do kernel (`kernel-paging`) já implementada, é possível fechar essa lacuna: mapear uma stack dedicada do kernel com uma guard page logo abaixo, de forma que um estouro de stack cause uma falha de página imediata e previsível, bem mais próxima do topo da stack, em vez de corrupção silenciosa.

## What Changes

- Mapear uma stack dedicada do kernel (tamanho fixo, ex.: 64 KiB) em uma faixa virtual reservada, usando `kernel-paging` e `kernel-physical-memory`, com uma página deliberadamente não mapeada imediatamente abaixo do seu limite inferior (a guard page).
- Trocar a stack ativa da CPU para essa nova stack logo após o alocador de frames físicos e a paginação estarem prontos (as primeiras etapas do boot, que precisam rodar antes de qualquer alocador existir, continuam na stack que o Limine fornece — ver design.md para o racional dessa fronteira).
- A partir da troca, qualquer estouro de stack do kernel (ex.: recursão profunda demais, uma função usando mais stack do que deveria) SHALL resultar em uma falha de página imediata e diagnosticável, em vez de corromper memória adjacente silenciosamente.

## Capabilities

### New Capabilities
- `kernel-stack-protection`: a stack própria do kernel, mapeada com uma guard page que transforma um estouro de stack em uma falha de página imediata e diagnosticável, em vez de corrupção silenciosa de memória adjacente.

### Modified Capabilities
- `kernel-interrupts`: o handler de page fault (`#PF`) passa a executar em sua própria stack dedicada via IST (mesmo mecanismo já usado pelo double fault), para continuar sendo entregue de forma confiável quando a causa for um estouro da stack do kernel. Descoberto como necessário durante a implementação desta change (ver design.md — "Page fault também precisa de stack dedicada (IST)"): sem isso, a CPU não consegue empilhar o frame de exceção do `#PF` numa stack já invadida pela guard page, e a falha escala direto para double fault sem o handler de `#PF` nunca rodar — violando o requisito desta própria change de que o estouro resulte em `#PF`, não em `#DF`. O comportamento observável do handler (formato do log, decisão de haltar) não muda; apenas o mecanismo de entrega da exceção.
- O contrato de halt de `kernel-boot` continua funcionando exatamente como já especificado.

## Impact

- **Novo módulo no crate `kernel`**: a stack dedicada e sua guard page (ex.: `kernel::stack`), usando `kernel::paging`/`kernel::phys` já existentes.
- **Mudança de comportamento interno em `kernel_main`**: depois que o alocador de frames físicos e a paginação estiverem prontos, a execução passa a rodar sobre a nova stack em vez da fornecida pelo Limine. Mudança interna, não afeta nenhum comportamento de boot já especificado.
- **Fecha o know issue** registrado em `KNOW_ISSUES.md` ("Ausência de guard page na stack principal do kernel") para toda a execução a partir do ponto da troca (a fase inicial de boot, antes de o alocador de frames existir, continua fora do escopo desta proteção — ver Non-Goals em design.md).
- **Nenhuma dependência externa nova esperada.**
