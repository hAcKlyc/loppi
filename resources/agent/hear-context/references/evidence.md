# 证据

摘要、topics、key_points、mentioned_objects 是基于粗转的派生理解，用于定位与概览。决定、承诺、数字和直接引语应核对原文。理解缺失时按原文回答，不为获取字段自动重新转写。

best_available 优先当前有效精转和人工修正文；未完整覆盖的粗转保留并标记 overlaps_precise。不要把重叠文本当成重复发生。粗转引用始终解析为原始粗转；可另读同时间范围精转作对照。

person_id 与 identity_origin 表示当前发言人归属。microphone 不能自动等同用户；system 也不必然是另一人。提及某人不等于这个人在发言。

保留事件时间、来源和 ref。ref 报 stale/unavailable 时不继续引用已缓存的旧句子。重新检索可取得当前有效版本。

coverage 只描述记录。未录、待处理、清理、无匹配分开解释；没有后续记录不能推出没有执行。文字可能仍在而原声已清理。

截图 text 可能包含图像摘要和识别文字，screenshot 字段标明来源与分界；摘要仍是派生理解。text_available=false 表示现场记录存在但文字不可用，不能当成没有截图。
