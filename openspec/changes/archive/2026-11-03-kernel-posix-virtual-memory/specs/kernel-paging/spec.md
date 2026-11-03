# Spec Delta

## ADDED Requirements

### Requirement: Alteração de Permissões de Página Já Mapeada
O kernel SHALL permitir alterar as permissões de acesso de uma página virtual já mapeada, preservando o frame físico atualmente mapeado para ela, sem exigir um desmapeamento (e remapeamento) intermediário.

#### Scenario: Permissões alteradas preservam o frame físico mapeado
- **WHEN** as permissões de uma página já mapeada são alteradas (ex.: remover a permissão de escrita)
- **THEN** o mesmo frame físico continua mapeado para aquele endereço virtual, e acessos subsequentes respeitam exatamente as novas permissões

#### Scenario: Alterar permissões de uma página não mapeada é rejeitada
- **WHEN** é solicitada a alteração de permissões de uma página virtual que não está mapeada
- **THEN** a operação SHALL ser rejeitada em vez de criar um mapeamento novo silenciosamente

#### Scenario: Alteração de permissões não depende de uma entrada de TLB obsoleta
- **WHEN** as permissões de uma página já mapeada são alteradas
- **THEN** um acesso imediatamente seguinte a essa página respeita as novas permissões, sem exigir nenhuma ação adicional do chamador para invalidar uma tradução antiga em cache
