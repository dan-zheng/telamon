for (pos, &{{#if set.arg~}}(arg, obj){{else}}obj{{/if~}})
    in {{>set.new_objs def=set objs="new_objs"}}.iter().enumerate() {
    {{#if set.arg~}}
        let arg = {{>set.item_getter def=set.arg id="arg"}};
    {{/if~}}
    let obj = {{>set.item_getter def=set id="obj" var="arg"}};
    {{#each arg_conflicts~}}
        {{>conflict var="arg" is_triangular=false}}
    {{/each~}}
    {{#each loop_nest.levels~}}
        for {{this.[0]}} in {{>set.iterator this.[1]}} {
            {{~#each this.[2]~}}
                {{>conflict var=../this.[0]}}
            {{~/each~}}
    {{~/each~}}
    {{#if set.arg~}}
        if let Some(arg) = Some(arg)
            {{#each arg_from_superset}}
                .and_then(|v| {{>set.from_superset this item="v"}})
            {{/each}}
        {
    {{/if~}}
    {{>@partial-block ../this}}
    {{#if set.arg~}} } {{/if~}}
    {{#each loop_nest.levels}} } {{/each}}
}
