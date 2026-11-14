use super::{models::*, network, optional_text, text, Core};
use serde_json::Value;

fn sha(value:&str,label:&str,size:usize)->Result<String,String>{
    if value.len()!=size||!value.bytes().all(|b|b.is_ascii_hexdigit()){return Err(format!("{label}须为{size}位十六进制值"));}
    Ok(value.to_ascii_lowercase())
}
fn safe_agent_report(value:String)->String {
    let lower=value.to_ascii_lowercase();
    if ["authorization", "bearer ", "github_pat_", "ghp_", "gho_", "access_token", "api_key", "token=", "cookie:", "headers:"].iter().any(|s|lower.contains(s)) {
        return "Agent 报告含敏感字段，已隐藏；请在外部构建记录中查看".into();
    }
    for word in value.split_whitespace() {
        if let Ok(url)=url::Url::parse(word) {
            if ["http","https"].contains(&url.scheme())&&(!url.username().is_empty()||url.as_str().split("://").nth(1).is_some_and(|part|part.split('/').next().is_some_and(|authority|authority.contains('@')))||url.query().is_some()) {
                return "Agent 报告含带凭据或查询参数的地址，已隐藏".into();
            }
        }
    }
    value
}
pub fn transition(from:&str,to:&str)->bool{
    matches!((from,to),("queued","claimed")|("queued","cancelled")|("claimed","running")|("claimed","failed")|("claimed","cancelled")|("running","running")|("running","succeeded")|("running","failed")|("running","cancelled"))
}
impl Core{
    pub(crate) async fn create_build_task(&self,payload:&Value)->Result<BuildTask,String>{
        let p=self.store.project(&text(payload,"projectId",128)?)?;
        let source_ref=optional_text(payload,"sourceRef",200)?.unwrap_or_else(||p.default_branch.clone());
        if source_ref.chars().any(char::is_control){return Err("源码引用不能包含控制字符".into());}
        let metadata=self.github.api(&p.owner,&p.repo,&[],None).await?;
        if metadata.get("private").and_then(Value::as_bool)!=Some(false){return Err("只支持公开项目的编译任务".into());}
        let commit=self.github.head(&p.owner,&p.repo,&source_ref).await?;
        let instructions=optional_text(payload,"instructions",16000)?.unwrap_or_else(||
            "请为固定的源码提交构建 Windows x64 版本，在授权环境中审查构建步骤，提交 GitHub 发行或 Actions 结果地址。此任务不会在 ReleaseHub 中执行脚本或安装软件。".into());
        let time=now();let task=BuildTask{id:uuid::Uuid::new_v4().to_string(),project_id:p.id,project_name:p.name,
            repository:format!("{}/{}",p.owner,p.repo),source_ref,source_commit:Some(commit),target:"windows-x64".into(),instructions,
            status:"queued".into(),agent_name:None,progress:None,artifact_url:None,artifact_sha256:None,error:None,created_at:time.clone(),updated_at:time};
        self.store.put_task(&task)?;self.changed();Ok(task)
    }
    pub(crate) fn claim_build_task(&self,id:&str,agent_name:&str)->Result<BuildTask,String>{
        let name=bounded_text(agent_name,100,"Agent 名称")?;
        let task=self.store.mutate("tasks",id,|t:&mut BuildTask|{
            if !transition(&t.status,"claimed"){return Err("任务已被领取或结束，请刷新列表".into());}
            t.status="claimed".into();t.agent_name=Some(name);t.updated_at=now();Ok(())
        })?;self.changed();Ok(task)
    }
    pub(crate) fn cancel_build_task(&self,id:&str)->Result<BuildTask,String>{
        let task=self.store.mutate("tasks",id,|t:&mut BuildTask|{
            if t.status=="cancelled"{return Ok(());}
            if !transition(&t.status,"cancelled"){return Err("已结束的任务不能取消".into());}
            t.status="cancelled".into();t.progress=Some("任务已取消；外部 Agent 需自行停止其构建操作".into());t.updated_at=now();Ok(())
        })?;self.changed();Ok(task)
    }
    pub(crate) fn update_build_task(&self,payload:&Value)->Result<BuildTask,String>{
        let id=text(payload,"id",128)?;let status=text(payload,"status",30)?;
        if !["running","succeeded","failed"].contains(&status.as_str()){return Err("任务状态无效".into());}
        let progress=optional_text(payload,"progress",4000)?.map(safe_agent_report);
        let error=optional_text(payload,"error",2000)?.map(safe_agent_report);
        let artifact=optional_text(payload,"artifactUrl",4096)?;
        let artifact_sha=optional_text(payload,"artifactSha256",64)?.map(|v|sha(&v,"文件 SHA-256",64)).transpose()?;
        let source_commit=optional_text(payload,"sourceCommit",40)?.map(|v|sha(&v,"源码提交",40)).transpose()?;
        let task=self.store.mutate("tasks",&id,|t:&mut BuildTask|{
            if !transition(&t.status,&status){return Err("任务状态变更无效，请先领取并开始任务".into());}
            if source_commit.as_ref().is_some_and(|c|t.source_commit.as_ref()!=Some(c)){return Err("源码提交与任务固定版本不一致".into());}
            if let Some(url)=&artifact{network::github_build_result_url(url)?;}
            if status=="succeeded"{
                if artifact.is_none()||t.source_commit.is_none()||source_commit.is_none(){return Err("成功结果必须包含 GitHub 结果地址和固定源码提交".into());}
                t.artifact_url=artifact;t.artifact_sha256=artifact_sha;t.error=None;
                t.progress=Some(format!("外部 Agent 编译完成（非官方发行），不会自动安装{}",
                    progress.map(|text|format!("：{text}")).unwrap_or_default()));
            }else{
                if artifact.is_some()||artifact_sha.is_some(){return Err("只有成功任务可以提交构建结果".into());}
                if let Some(v)=progress{t.progress=Some(v);}
                t.error=if status=="failed"{Some(error.unwrap_or_else(||"Agent 构建失败，请查看外部构建记录".into()))}else{None};
            }
            t.status=status;t.updated_at=now();Ok(())
        })?;self.changed();Ok(task)
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use std::sync::Arc;
    #[test]fn terminal_tasks_do_not_revive(){assert!(!transition("cancelled","running"));assert!(!transition("succeeded","running"));assert!(!transition("queued","succeeded"));assert!(transition("claimed","running"));assert!(transition("running","succeeded"));}
    #[test]fn claim_is_atomic_and_result_is_bound_to_source(){
        let dir=tempfile::tempdir().unwrap();let core=Core::new(dir.path().into(),true,Arc::new(|_,_|{})).unwrap();let time=now();
        let task=BuildTask{id:"test-task".into(),project_id:"project".into(),project_name:"project".into(),repository:"owner/repo".into(),
            source_ref:"main".into(),source_commit:Some("a".repeat(40)),target:"windows-x64".into(),instructions:"仅构建".into(),status:"queued".into(),
            agent_name:None,progress:None,artifact_url:None,artifact_sha256:None,error:None,created_at:time.clone(),updated_at:time};
        core.store.put_task(&task).unwrap();core.claim_build_task(&task.id,"agent-one").unwrap();assert!(core.claim_build_task(&task.id,"agent-two").is_err());
        core.update_build_task(&serde_json::json!({"id":task.id,"status":"running"})).unwrap();
        let fork_url="https://github.com/my-account/my-builds/actions/runs/12";
        assert!(core.update_build_task(&serde_json::json!({"id":task.id,"status":"succeeded","sourceCommit":"b".repeat(40),"artifactUrl":fork_url})).is_err());
        assert!(core.update_build_task(&serde_json::json!({"id":task.id,"status":"succeeded","artifactUrl":fork_url})).is_err());
        assert!(core.update_build_task(&serde_json::json!({"id":task.id,"status":"succeeded","sourceCommit":"a".repeat(40),"artifactUrl":"https://evil.example/file"})).is_err());
        let done=core.update_build_task(&serde_json::json!({"id":task.id,"status":"succeeded","sourceCommit":"a".repeat(40),"artifactUrl":fork_url,"progress":"构建完成"})).unwrap();assert_eq!(done.status,"succeeded");
        assert_eq!(done.source_commit,Some("a".repeat(40)));assert_eq!(done.artifact_url.as_deref(),Some(fork_url));
        let progress=done.progress.unwrap();assert!(progress.contains("不会自动安装"));assert!(progress.contains("非官方"));
        assert!(core.store.downloads().unwrap().is_empty());assert_eq!(std::fs::read_dir(core.data_dir.join("apps")).unwrap().count(),0);
    }
}
