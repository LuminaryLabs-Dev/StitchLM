#include "llama.h"
#include "ggml-backend.h"
#include <vector>
#include <string>
#include <fstream>
#include <filesystem>
#include <algorithm>
#include <iostream>
struct Trace { std::string dir; int position=0; bool enabled=false; };
static bool capture(ggml_tensor *t, bool ask, void *data) {
 auto *s=static_cast<Trace*>(data);std::string name=ggml_get_name(t);
 bool selected=s->enabled && t->type==GGML_TYPE_F32 && (name.find("l_out")==0 || name.find("attn_residual")==0 || name.find("conv_output_silu")==0 || name.find("final_output")==0 || name.find("result_norm")==0 || name.find("model.input_embed")==0);
 if(ask)return selected;if(!selected)return true;
 std::string dir=s->dir+"/trace-"+std::to_string(s->position);std::filesystem::create_directories(dir);
 std::vector<char> b(ggml_nbytes(t));ggml_backend_tensor_get(t,b.data(),0,b.size());std::ofstream f(dir+"/"+name+".f32",std::ios::binary);f.write(b.data(),b.size());return true;
}
int main(int argc,char**argv){
 if(argc!=5){std::cerr<<"MODEL PROMPT STEPS OUTPUT\n";return 2;}std::string prompt=argv[2],out=argv[4];int steps=std::stoi(argv[3]);std::filesystem::create_directories(out);
 llama_backend_init();auto mp=llama_model_default_params();mp.n_gpu_layers=0;mp.use_mmap=true;
 auto *m=llama_model_load_from_file(argv[1],mp);if(!m)return 3;auto *v=llama_model_get_vocab(m);
 int n=-llama_tokenize(v,prompt.data(),prompt.size(),nullptr,0,false,true);std::vector<llama_token> ids(n);if(llama_tokenize(v,prompt.data(),prompt.size(),ids.data(),ids.size(),false,true)!=n)return 4;
 {std::ofstream f(out+"/input.txt");for(auto id:ids)f<<id<<"\n";}
 Trace trace{out,0,getenv("STITCH_TRACE")!=nullptr};auto cp=llama_context_default_params();cp.n_ctx=128;cp.n_batch=128;cp.n_ubatch=1;cp.n_threads=2;cp.n_threads_batch=2;cp.type_k=GGML_TYPE_F32;cp.type_v=GGML_TYPE_F32;cp.cb_eval=capture;cp.cb_eval_user_data=&trace;
 auto*c=llama_init_from_model(m,cp);if(!c)return 5;int nv=llama_vocab_n_tokens(v);std::ofstream gen(out+"/generated.txt");
 for(int pos=0;pos<n+steps-1;pos++){trace.position=pos;llama_token input=ids[pos];auto batch=llama_batch_get_one(&input,1);if(llama_decode(c,batch))return 6;float*logits=llama_get_logits_ith(c,-1);if(!logits)return 7;
 std::ofstream f(out+"/logits-"+std::to_string(pos)+".f32",std::ios::binary);f.write((char*)logits,nv*sizeof(float));int next=std::max_element(logits,logits+nv)-logits;
 std::cerr<<"POSITION "<<pos<<" INPUT "<<input<<" NEXT "<<next<<"\n";if(pos>=n-1){gen<<next<<"\n";gen.flush();ids.push_back(next);}}
 llama_free(c);llama_model_free(m);llama_backend_free();return 0;
}
