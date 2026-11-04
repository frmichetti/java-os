# Tasks

## 1. Atualizar a documentação de arquitetura com a decisão

- [x] 1.1 Reescrever o parágrafo de "Risco não previsto originalmente, registrado sem decisão fechada" na seção 6 de `docs/architecture/overview.md` ("Estratégia de Portabilidade do OpenJDK"): substituir pela decisão tomada (build estático do JDK/Leyden rejeitado — dependência específica de `/proc/self/fd`, exclusiva do Linux; linker ELF dinâmico mínimo no kernel adotado) e registrar o achado de que um carregador de executável ELF estático é um pré-requisito separado, ainda inexistente — ver design.md ("Decisions"/"Context"). Verificar lendo o parágrafo reescrito.
- [x] 1.2 Atualizar o item 8 do roadmap (seção 15 de `docs/architecture/overview.md`): registrar esta change como a decisão que fecha o risco de carregamento dinâmico, e nomear explicitamente a próxima sub-change concreta (carregador de executável ELF estático), antes do linker dinâmico. Verificar lendo o item atualizado.

## Workflow follow-up

- Arquivar esta change após a aprovação do usuário para a decisão documentada.
