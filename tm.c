
// (1) g++ -o st tm.c `mysql_config --cflags --libs`
// (2) crontab -e   设置分钟定时任务    */1 * * * * /home/z/my_files/st 1 >> /home/z/my_files/A.txt
// (3) sudo systemctl start cron     启动分钟定时任务 
// (4) stdbuf -oL tail -f A.txt

#include <stdio.h>
#include <time.h>
#include <string.h>
#include <stdlib.h>
#include <mysql/mysql.h>

//-------------------------------------------------台站号，地名列表 现：30
struct STA {short ik;char id[8];char sv[32];};
static struct STA mSTA[] =
	{
		{ 0,"50936","BFBC_吉林白城_华云"},{ 1,"50968","BTSZ_黑龙江尚志_华云" },{ 2,"53399","BVZB_河北张北_华云"},{ 3,"53942","BFLC_陕西洛川_华云"},{ 4,"54333","BGXM_辽宁新民_华云"},
		{ 5,"54416","BJMY_北京密云_华云"},{ 6,"54808","BTSX_山东莘县_华云"   },{ 7,"56173","BXHY_四川红原_华云"},{ 8,"56312","BFLR_西藏林芝_华云"},{ 9,"57633","BVYA_重庆酉阳_华云"},
		{10,"57958","BGYA_广西雁山_华云"},{11,"58005","BTSQ_河南商丘_华云"   },{12,"58457","BEHZ_浙江杭州_华云"},{13,"58737","BTJO_福建建瓯_华云"},{14,"52983","BTYZ_甘肃榆中_天津"},
		{15,"53817","BFGU_宁夏固原_天津"},{16,"51358","BTLS_新疆乌兰乌苏_无锡"},{17,"52754","BUGC_青海刚察_无锡"},{18,"52856","BFGH_青海共和_无锡"},{19,"53963","BGHM_山西侯马_无锡"},
		{20,"56739","BTTC_云南腾冲_无锡"},{21,"57251","BTYX_湖北郧西_无锡"   },{22,"57793","BZYC_江西宜春_无锡"},{23,"57832","BHCB_贵州三穗_无锡"},{24,"57874","BFHA_湖南常宁_无锡"},
		{25,"58141","BTHY_江苏淮安_无锡"},{26,"58362","BCSH_上海宝山_无锡"   },{27,"58437","BXHS_安徽黄山_无锡"},{28,"59758","BEHK_海南海口_无锡"},{29,"59294","GDZC_广州增城_广东"},   
		{29,"52737","BFDH_青海徳令哈_无锡"},{30,"57914","BIAD_贵州花溪_无锡"   },
		{32,"HY001","...._......._...."},   
		{33,"TJ001","...._......._...."}
	};
#define STAn (sizeof(mSTA)/sizeof(STA))
//------------------------------------------ST包 信息解释
char alarmMsg[1024];
void getALM(const char *I,const char *V)
    {
    sprintf(alarmMsg,"[?%s=%s]",I,V);
    if(I[0]=='a')
        {
        if(strcmp(I,"aCF")==0) 
            {    
            switch(V[0])
                {
                case '0':sprintf(alarmMsg,"存储卡_:正常");break;    
                case '1':sprintf(alarmMsg,"存储卡_:无卡");break;    
                case '2':sprintf(alarmMsg,"存储卡_:故障");break;    
                }
            }
        else
        if(strcmp(I,"aDOOR")==0) 
            {    
            switch(V[0])
                {
                case '0':sprintf(alarmMsg,"机箱门_:正常");break;    
                case '1':sprintf(alarmMsg,"机箱门_:异常");break;    
                }
            }
        else
        if(strcmp(I,"aLID")==0) 
            {    
            switch(V[0])
                {
                case '0':sprintf(alarmMsg,"酸雨盖_:正常");break;    
                case '1':sprintf(alarmMsg,"酸雨盖_:开启");break;    
                }
            }
        else
        if(strcmp(I,"aLEVEL")==0) 
            {    
            switch(V[0])
                {
                case '0':sprintf(alarmMsg,"水位_:正常");break;    
                case '3':sprintf(alarmMsg,"水位_:偏高");break;    
                case '4':sprintf(alarmMsg,"水位_:偏低");break;    
                }
            }
        else
        if(strcmp(I,"aSWITCH")==0) 
            {    
            switch(V[0])
                {
                case 'O':     if(V[1]=='N') sprintf(alarmMsg,"水开关_:开启");
                         else if(V[1]=='F') sprintf(alarmMsg,"水开关_:关闭");break;
                case 'N':                   sprintf(alarmMsg,"水开关_:无设备");break;    
                }
            }
        else
        if(strcmp(I,"aSWITCHA")==0) 
            {    
            switch(V[0])
                {
                case '0':sprintf(alarmMsg,"加排水_:正常");break;    
                case '1':sprintf(alarmMsg,"加排水_:异常");break;    
                case '2':sprintf(alarmMsg,"加排水_:故障");break;    
                case '3':sprintf(alarmMsg,"加排水_:加水");break;    
                case '4':sprintf(alarmMsg,"加排水_:排水");break;    
                case '5':sprintf(alarmMsg,"加排水_:维护");break;    
                }
            }
        else
        if(strcmp(I,"aTILT")==0) 
            {
            sprintf(alarmMsg,"北斗设备倾斜角_:%s度",V);
            }
        }
    else 
    if(//-------------C.2 符合这个表的项目
              I[1]==0                                           ||
            ( I[2]==0 && I[0]=='y' && I[1] >='A' && I[1]<='B')  ||
            ( I[2]==0 && I[0]=='u' && I[1] >='A' && I[1]<='C') 
           )
        {
        if(I[1]==0)
            {    
            switch(I[0])
                {
                case 'a':sprintf(alarmMsg,"其他工作_");break; 
                case 'q':sprintf(alarmMsg,"分钟数据_");break; 
                case 'r':sprintf(alarmMsg,"采样数据_");break; 
                case 's':sprintf(alarmMsg,"污染状态_");break; 
                case 't':sprintf(alarmMsg,"通讯状态_");break; 
                case 'u':sprintf(alarmMsg,"通风部件_");break; 
                case 'v':sprintf(alarmMsg,"加热部件_");break; 
                case 'w':sprintf(alarmMsg,"温度状态_");break; 
                case 'x':sprintf(alarmMsg,"供电状态_");break; 
                case 'y':sprintf(alarmMsg,"测量仪_");break; 
                case 'z':sprintf(alarmMsg,"设备自检_");break; 
                }
            }    
        else
            {
            if( I[0]=='y') 
                {
                switch(I[1])
                    {
                    case 'A':sprintf(alarmMsg,"测量部分自检_");break;
                    case 'B':sprintf(alarmMsg,"辅助设备自检_");break;    
                    }    
                }
            else    
            if( I[0]=='u') 
                {
                switch(I[1])
                    {
                    case 'A':sprintf(alarmMsg,"设备通风_");break;
                    case 'B':sprintf(alarmMsg,"发射器通风_");break;
                    case 'C':sprintf(alarmMsg,"接收器通风_");break;    
                    }    
                }
            }
        switch(V[0])
            {
            case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break; 
            case '1':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":异常");break; 
            case '2':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":故障（未检测到）");break; 
            case '3':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":偏高");break; 
            case '4':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":偏低");break; 
            case '5':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":超上限");break; 
            case '6':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":超下限");break; 
            case '7':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":预留");break; 
            case '8':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":预留");break; 
            case '9':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":未检查");break; 
            case 'N':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":关闭或无配置");break; 
            }    
        }   
    else if(I[2]==0)// 下面是 非 C.2 表的情况
        {
        switch(I[0])
            {
            case 'y':
            if( (I[1]>='C'&& I[1]<='H') || I[1]=='J' )
                {
                switch(I[1])
                    {
                    case 'C':sprintf(alarmMsg,"翻斗雨量_");break;
                    case 'D':sprintf(alarmMsg,"筒口_");break;
                    case 'E':sprintf(alarmMsg,"上翻斗_");break;
                    case 'F':sprintf(alarmMsg,"计数翻斗_");break;
                    case 'G':sprintf(alarmMsg,"计数翻斗1_");break;
                    case 'H':sprintf(alarmMsg,"计数翻斗2_");break;
                    case 'J':sprintf(alarmMsg,"颗粒物谱传感器_");break;
                    }   
                switch(V[0])
                    {
                    case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break;    
                    case '1':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":异常");break;    
                    case '2':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":堵塞");break;   
                    }
                }
            else
            if( I[1]=='I' )
                {
                sprintf(alarmMsg,"筒口_");
                switch(V[0])
                    {
                    case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break;    
                    case '2':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":故障");break;   
                    }
                }    
            else
            if( I[1]>='K'&& I[1]<='M')
                {
                switch(I[1])
                    {
                    case 'K':sprintf(alarmMsg,"鱼眼相机_");break;
                    case 'L':sprintf(alarmMsg,"普通相机1_");break;
                    case 'M':sprintf(alarmMsg,"普通相机2_");break;
                    }   
                switch(V[0])
                    {
                    case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break;    
                    case '1':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":可连接但无法拍照");break;    
                    case '2':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":无法连接");break;   
                    }
                }
            else
            if( I[1]=='N')
                {
                sprintf(alarmMsg,"智能电源_");
                switch(V[1])
                    {
                    case 'N':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":电源开启");break;    
                    case 'F':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":电源关闭");break;    
                    }
                }
            break;
            case 'x':
            {
            switch(I[1])
                {
                case 'A':sprintf(alarmMsg,"供电类型_:%s",V);break;
                case 'B':sprintf(alarmMsg,"外接电源电压_:%s伏",V);break;
                case 'C':sprintf(alarmMsg,"蓄电池电压_:%s伏",V);break;
                case 'D':sprintf(alarmMsg,"设备供电电压_:%s伏",V);break;
                case 'E':sprintf(alarmMsg,"当前主板电压值_:%s伏",V);break;
                case 'F':sprintf(alarmMsg,"当前工作电流_:%s毫安",V);break;
                case 'G':sprintf(alarmMsg,"加热电源电压值_:%s伏",V);break;
                case 'H':sprintf(alarmMsg,"蓄电池电量_:%s/100",V);break;
                }   
            }
            break;
            case 'w':
            {
            switch(I[1])
                {
                case 'A':sprintf(alarmMsg,"电路板温度_:%s℃",V);break;
                case 'B':sprintf(alarmMsg,"探测器温度_:%s℃",V);break;
                case 'C':sprintf(alarmMsg,"腔体温度_:%s℃",V);break;
                case 'D':sprintf(alarmMsg,"恒温器温度_:%s℃",V);break;
                case 'E':sprintf(alarmMsg,"机箱温度_:%s℃",V);break;
                }   
            }
            break;
            case 'v':
            {
            switch(I[1])
                {
                case 'A':sprintf(alarmMsg,"设备加热开关状态_:%s",V);break;
                case 'B':sprintf(alarmMsg,"发射器加热开关状态_:%s",V);break;
                case 'C':sprintf(alarmMsg,"接收器加热开关状态_:%s",V);break;
                case 'D':sprintf(alarmMsg,"相机加热开关状态_:%s",V);break;
                case 'E':sprintf(alarmMsg,"鱼眼摄像机加热开关状态_:%s",V);break;
                case 'F':sprintf(alarmMsg,"普通摄像机1加热开关状态_:%s",V);break;
                case 'G':sprintf(alarmMsg,"普通摄像机2加热开关状态_:%s",V);break;
                case 'H':sprintf(alarmMsg,"风速加热开关状态_:%s",V);break;
                case 'I':sprintf(alarmMsg,"风向加热开关状态_:%s",V);break;
                }   
            }
            break;
            case 'u':
            {
            switch(I[1])
                {
                case 'D':sprintf(alarmMsg,"通风罩通风速度_:%s(m/s)",V);break;
                case 'E':sprintf(alarmMsg,"通风罩转速_:%s(r/min)",V);break;
                }   
            }
            break;
            case 't':
            {
            switch(I[1])
                {
                case 'A':sprintf(alarmMsg,"设备到智能集成处理器通信状态_");break;
                case 'B':sprintf(alarmMsg,"总线状态_");break;
                case 'C':sprintf(alarmMsg,"串口通信状态_");break;
                case 'D':sprintf(alarmMsg,"网口通信状态_");break;
                case 'E':sprintf(alarmMsg,"鱼眼相机网口通信状态_");break;
                case 'F':sprintf(alarmMsg,"普通相机1网口通信状态_");break;
                case 'G':sprintf(alarmMsg,"普通相机2网口通信状态_");break;
                }   
            switch(V[0])
                {
                case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break;    
                case '1':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":故障");break;    
                case '2':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":未启用");break;   
                }
            }
            break;
            case 's':
            {
            switch(I[1])
                {
                case 'A':sprintf(alarmMsg,"窗口_");break;    
                case 'B':sprintf(alarmMsg,"探测器_");break;    
                case 'C':sprintf(alarmMsg,"镜头_");break;    
                case 'D':sprintf(alarmMsg,"鱼眼镜头_");break;    
                case 'E':sprintf(alarmMsg,"摄像头1_");break;    
                case 'F':sprintf(alarmMsg,"摄像头2_");break;    
                case 'G':sprintf(alarmMsg,"降水现象仪1窗口_");break;    
                case 'H':sprintf(alarmMsg,"降水现象仪2窗口_");break;    
                }    
            switch(V[0])
                {
                case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break;    
                case '1':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":一般污染");break;    
                case '2':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":严重污染");break;   
                }
            }
            break;
            case 'r':
            {
            switch(I[1])
                    {
                    case 'A':sprintf(alarmMsg,"分钟采样值超上限次数_:%s",V);break;
                    case 'B':sprintf(alarmMsg,"分钟采样值超下限次数_:%s",V);break;
                    case 'C':sprintf(alarmMsg,"分钟采样值跳变超限次数_:%s",V);break;
                    }   
            }
            break;
            case 'q':
            {
            switch(I[1])
                {
                case 'A':sprintf(alarmMsg,"当前设备输出分钟数据值_不超上限_");break;    
                case 'B':sprintf(alarmMsg,"当前设备输出分钟数据值_不超下限_");break;    
                case 'C':sprintf(alarmMsg,"当前设备输出分钟数据变化率_不超限_");break;    
                case 'D':sprintf(alarmMsg,"当前设备输出分钟数据(存疑)不超限_");break;    
                case 'E':sprintf(alarmMsg,"当前设备输出分钟数据达到最小变化率_");break;    
                }    
            switch(V[0])
                {
                case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":是的（正常）");break;    
                case '1':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":不是（错误）");break;    
                }
            }
            break;
            }
        } 
    else if(I[3]==0)
        {
        //xEA xFA xGA
        if(I[0]=='x' && I[2]=='A')
            {
            switch(I[1])
                {
                case 'E':sprintf(alarmMsg,"主板电压_");break;    
                case 'F':sprintf(alarmMsg,"工作电流_");break;    
                case 'G':sprintf(alarmMsg,"加热电压_");break;    
                }    
            switch(V[0])
                {
                case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break;    
                case '3':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":偏高");break;    
                case '4':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":偏低");break;   
                }
            }
        //wAA wCA
        else    
        if(I[0]=='w' && I[2]=='A')
            {
            switch(I[1])
                {
                case 'A':sprintf(alarmMsg,"电路板温度_");break;    
                case 'C':sprintf(alarmMsg,"腔体温度_");break;    
                }    
            switch(V[0])
                {
                case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break;    
                case '3':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":偏高");break;    
                case '4':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":偏低");break;   
                }
            }
        //vAA ... vKA
        else    
        if(I[0]=='v' && I[2]=='A')
            {
            switch(I[1])
                {
                case 'A':sprintf(alarmMsg,"设备加热_");break;    
                case 'B':sprintf(alarmMsg,"发射器加热_");break;    
                case 'C':sprintf(alarmMsg,"接收器加热_");break;    
                case 'D':sprintf(alarmMsg,"相机加热_");break;    
                case 'E':sprintf(alarmMsg,"鱼眼相机加热_");break;    
                case 'F':sprintf(alarmMsg,"摄像机1加热_");break;    
                case 'G':sprintf(alarmMsg,"摄像机2加热_");break;    
                case 'H':sprintf(alarmMsg,"风速加热_");break;    
                case 'I':sprintf(alarmMsg,"风向加热_");break;    
                case 'J':sprintf(alarmMsg,"降水现象仪通道1加热_");break;    
                case 'K':sprintf(alarmMsg,"降水现象仪通道2加热_");break;    
                }    
            switch(V[0])
                {
                case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break;    
                case '1':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":异常");break;    
                case '2':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":故障");break;   
                case '3':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":偏高");break;    
                case '4':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":偏低");break;    
                case '5':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":停止");break;   
                }
            }
        //uDA uEA
        else    
        if(I[0]=='u' && I[2]=='A')
            {
            switch(I[1])
                {
                case 'D':sprintf(alarmMsg,"通风罩通风_");break;    
                case 'E':sprintf(alarmMsg,"通风罩转速_");break;    
                }    
            switch(V[0])
                {
                case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break;    
                case '1':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":异常");break;    
                case '2':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":故障");break;   
                case '3':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":偏高");break;    
                case '4':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":偏低");break;    
                }
            }
        //tDA ... tDC
        else    
        if(I[0]=='t' && I[1]=='D')
            {
            switch(I[2])
                {
                case 'A':sprintf(alarmMsg,"鱼眼摄像机  网口_");break;    
                case 'B':sprintf(alarmMsg,"普通摄像机1 网口_");break;    
                case 'C':sprintf(alarmMsg,"普通摄像机1 网口_");break;    
                }    
            switch(V[0])
                {
                case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break;    
                case '1':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":故障");break;    
                case '2':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":未启用");break;   
                }
            }
        //tFA ... tFC
        else    
        if(I[0]=='t' && I[1]=='F')
            {
            switch(I[2])
                {
                case 'A':sprintf(alarmMsg,"无线信号强度_:%s dBm",V);break;    
                case 'B':sprintf(alarmMsg,"无线信号强度_:%s 级",V);break;    
                case 'C':sprintf(alarmMsg,"无线连接状态_");break;    
                }    
            if(I[2]=='C')
            switch(V[0])
                {
                case '0':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":正常");break;    
                case '7':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":物理链接断开");break;    
                case '8':sprintf((char *)&alarmMsg[strlen(alarmMsg)],":逻辑链路断开");break;   
                }
            }
        }
    sprintf((char *)&alarmMsg[strlen(alarmMsg)],"# ");
    }
//---------------------------------------- 0正常项检测
bool isKIT(char *m)
    {
    int z=0;
    if(m[1]==0) 
        {
        if(m[0]=='a' || (m[0]>= 'q'&& m[0]<='z'))    z=1;
        }
    else if(m[2]==0)
        {
             if(m[0]=='y' && m[1]>='A' && m[1]<='M') z=1; //N:ON OFF
        else if(m[0]=='s' && m[1]>='A' && m[1]<='H') z=1;
        else if(m[0]=='q' && m[1]>='A' && m[1]<='E') z=1;
        else if(m[0]=='u' && m[1]>='A' && m[1]<='C') z=1;
        else if(m[0]=='t' && m[1]>='A' && m[1]<='G' && m[1]!='D') z=1; // 忽略：tD
        }
    else if(m[3]==0)
        {
             if(m[0]=='x'&& m[1]>='E' && m[1]<='G' && m[2]=='A') z=1;//EFG
        else if(m[0]=='w'&&(m[1]=='A' || m[1]=='C')&& m[2]=='A') z=1;//
        else if(m[0]=='v'&& m[1]>='A' && m[1]<='K' && m[2]=='A') z=1;//ABCDEFGHIJK
        else if(m[0]=='w'&& m[1]=='A' &&              m[2]=='A') z=1;
        else if(m[0]=='u'&&(m[1]=='D' || m[1]=='E')&& m[2]=='A') z=1;
        else if(m[0]=='t'&& m[2]>='A' && m[1]<='C' && m[1]=='D') z=1;//ABC
        else if(m[0]=='t'&& m[1]=='F'              && m[2]=='C') z=1;//ABC
        }
    else if(strcmp(m,"aCF"     ) == 0 ) z=1;
    else if(strcmp(m,"aDOOR"   ) == 0 ) z=1;
    else if(strcmp(m,"aLID"    ) == 0 ) z=1;
    else if(strcmp(m,"aLEVEL"  ) == 0 ) z=1;
    else if(strcmp(m,"aSWITCHA") == 0 ) z=1;
    
    if(z) return true;
    else  return false;    
    }
//--------------------------------------------由台站号获取对应的序列号 0....
int getSid(char *num)
    {
    if(num != NULL) for(int n=0;n<STAn;n++) 
        {
    	if(strcmp(num,mSTA[n].id)==0) return n;
        }
    return -1;
    }
//---------------------------------------------------------对数据正常入库的台站，检测一个时次的 ST包
void sqlProST(MYSQL *conn, char *row0, char *row3) {
    char sqlcmd[2048];
    char bk3[16];
    char esc_row0[256];
    char esc_row3[256];
    mysql_real_escape_string(conn, esc_row0, row0, strlen(row0));
    mysql_real_escape_string(conn, esc_row3, row3, strlen(row3));
    snprintf(sqlcmd, sizeof(sqlcmd), "select data from data_st where station_num='%s' and data_time='%s';", esc_row0, esc_row3);

        int  ret=0;
        int  kcN=0;
        char erDm[2048];
        erDm[0]=0;
        bk3[0]=0;
        if (mysql_query(conn, sqlcmd) != 0) {printf("q: %s\n", mysql_error(conn));} else
            {//==0
            MYSQL_RES *result = mysql_store_result(conn);
            if (result == NULL ) {printf("q: %s\n", mysql_error(conn));} else
                {//!=NULL
                MYSQL_ROW row;
                while( (row= mysql_fetch_row(result)))
                    {
                    //row[0] only
                    //0        1       2     3       4   5  6              7
                    //DATADICK,V202201,52983,YHUMI00,N01,ST,20260609094100,z,0,rA,0,rB,0,rC,0,qA,0,qB,0,qC,0,qD,0,qE,0,tF,0,tFA,-50,tFB,4,tFC,0,wA,25.0,wAA,0,xA,DC,xE,12.6,xEA,0,xF,37,xFA,0,0343,ED

                    int  i,j,nt,pwi;
                    char itm[128][64];
                    //printf("\n%s\n",row[0]);
                    for(i=j=nt=0;i<strlen(row[0]);i++)
                        {
                        char c=row[0][i];
                        if(c==',' || c <' ') {itm[nt++][j]=0; j=0;}   
                                        else {
                                             if(nt==16 && j==0) pwi=i;
                                             itm[nt  ][j  ]=c;
                                             if(j<63) j++;
                                             }
                        }
                    //printf("%s\n",row[0]);
                    if(strcmp(itm[3],"YPOWR00")==0) //智能电源的ST包格式是 自定的 不符合字典！！！
                        {
                        int i,j;
                        char swk[2048];
                        for(j=i=0;i<2048;i++) 
                            {
                            if(row[0][i+pwi]==',') break;
                            swk[j++]=row[0][i+pwi];
                            swk[j]=0;
                            }
                    /* ===================================================XXXX 临时关闭 YPOWR00 的检测
                        if( !(swk[0]=='O' || swk[0]=='/') ) 
                            {
                            sprintf((char *)&erDm[strlen(erDm)],"\e[33m#%s_%s:\e[91m无开关表\e[37m ",itm[3],itm[4]);        
                            ret++;
                            }
                        else sprintf((char *)&erDm[strlen(erDm)],"#%s_%s:有开关表 ",itm[3],itm[4]);        
                    //*/
                        }
                    else                        // 下面按照字典解析
                        {
                        for(i=7;i<nt-2;i+=2) 
                            {
                            if(itm[i+1][1]==0 && itm[i+1][0]=='N') continue;//跳过关闭或没有配置的项目

                            if(itm[i+1][1]==0 && itm[i+1][0]=='-') continue;//===================================================XXXX 临时跳过
                            if(itm[i+1][1]==0 && itm[i+1][0]=='C') continue;//===================================================XXXX 临时跳过
                            if(itm[i+1][1]==0 && itm[i+1][0]=='/') continue;//===================================================XXXX 临时跳过

                            //getALM(itm[i],itm[i+1]);printf("\n%s",alarmMsg);        
                            if(isKIT(itm[i]) )//本项对应的数值为 0 是正常状态
                                {
                                kcN++;    
                                if( !( strcmp(itm[i+1],"0") == 0 || 
                                       strcmp(itm[i+1],"0:0:0:0:0:0:0:0:0:0") == 0 ) )
                                    {
                                    ret++;
                                    if( itm[i+1][0]>='0' && itm[i+1][0]<='9' ) 
                                        {
                                        getALM(itm[i],itm[i+1]);//sprintf((char *)&erDm[strlen(erDm)],"\e[33m#%s_%s:%s=%s \e[31m%s\e[37m\t",itm[3],itm[4],itm[i],itm[i+1],alarmMsg);        

                                        if(strcmp(bk3,itm[3])==0 ) sprintf((char *)&erDm[strlen(erDm)],"\e[33m_%s:\e[31m%s\e[37m "          ,itm[4],alarmMsg);   
                                                          else     sprintf((char *)&erDm[strlen(erDm)],"\e[33m#%s_%s:\e[31m%s\e[37m ",itm[3],itm[4],alarmMsg);   
                                        sprintf(bk3,"%s",itm[3]);     
                                        }
                                    else 
                                        {
                                        getALM(itm[i],itm[i+1]);    
                                        sprintf((char *)&erDm[strlen(erDm)],"(%s_%s:%s=%s:%s)\t",itm[3],itm[4],itm[i],itm[i+1],alarmMsg);    
                                        }
                                    }
                                }
                            }    
                        }
                    }
                }    
            } 

        if(ret==0) printf(" 检：%3d项，无报警 %s\n",kcN,erDm);   
             else  printf(" 检：%3d项，报警:%d %s\n",kcN,ret,erDm);   
        }
//---------------------------------------- m a i n -------------------------------------------------------------------------
int main(int argc, char *argv[]) {
    char cmd[4096];
    int tD;
    if (argc == 1) tD = 10;
    if (argc > 1) {
        sscanf(argv[1], "%d", &tD);
        if (tD < 1 || tD > 1440) tD = 10;
    }

    // 环境变量读取数据库配置，避免硬编码密码
    const char *db_host = getenv("DB_HOST") ? getenv("DB_HOST") : "10.10.1.59";
    const char *db_user = getenv("DB_USER") ? getenv("DB_USER") : "root";
    const char *db_pass = getenv("DB_PASSWORD") ? getenv("DB_PASSWORD") : "root";
    const char *db_name = getenv("DB_NAME") ? getenv("DB_NAME") : "cammoc_w";

    // 先连接数据库（mysql_real_escape_string 需要有效连接）
    MYSQL *conn = mysql_init(NULL);
    if (conn == NULL) {
        fprintf(stderr, "X0\n");
        return 1;
    }
    if (mysql_real_connect(conn, db_host, db_user, db_pass, db_name, 3306, NULL, 0) == NULL) {
        fprintf(stderr, "X1\n");
        mysql_close(conn);
        return 1;
    }

    // 连接后构建 SQL：使用转义 + snprintf 防止 SQL 注入和缓冲区溢出
    snprintf(cmd, sizeof(cmd),
        "select station_num,count(*),count(if(data_time>(now()-interval 6 minute),1,null))"
        ",min(data_time),max(data_time),count(distinct concat(device_type,device_nid))"
        "from data_st"
        "where receive_time>(now()- interval %d minute )"
        "and station_num in (", tD);
    for (int i = 0; i < STAn; i++) {
        char esc_id[64];
        mysql_real_escape_string(conn, esc_id, mSTA[i].id, strlen(mSTA[i].id));
        strncat(cmd, "'", sizeof(cmd) - strlen(cmd) - 1);
        strncat(cmd, esc_id, sizeof(cmd) - strlen(cmd) - 1);
        strncat(cmd, ",", sizeof(cmd) - strlen(cmd) - 1);
    }
    if (strlen(cmd) > 0) cmd[strlen(cmd) - 1] = ')';
    strncat(cmd, " group by station_num order by station_num;", sizeof(cmd) - strlen(cmd) - 1);

    //----------------begin time-------------------------------------------------------------------
    time_t current_time, end_time;
    struct tm *time_info;
    char time_string[100];
    time(&current_time);
    time_info = localtime(&current_time);
    strftime(time_string, sizeof(time_string), "%Y-%m-%d %H:%M:%S", time_info);
    printf("\n===================================================================================================\n");
    printf("%s ", time_string);
    printf("     From: %s:3306 => %s => data_st 最近 %d 分钟入库数据\n", db_host, db_name, tD);

    int Trec = 0;
    int isN = 0;

    if (mysql_query(conn, cmd) != 0) {
        fprintf(stderr, "x5: %s\n", mysql_error(conn));
    } else {
        // ==0
        //----------------end time-------------------------------------
        time(&end_time);
        printf("查询耗时：%d 秒\n", (int)difftime(end_time, current_time));

        MYSQL_RES *result = mysql_store_result(conn);
        if (result == NULL) {
            fprintf(stderr, "x4: %s\n", mysql_error(conn));
        } else {
            // !=NULL
            int nl = mysql_num_fields(result);
            MYSQL_ROW row;
            while ((row = mysql_fetch_row(result))) {
                if (row[3] != NULL) {
                    char Lbuf[10240];
                    int r1, r2, r5;
                    static char itm = 0, iii = 0;
                    int wik = getSid(row[0]);
                    if (iii == 0) printf("\n序号\t台站号\t入库数\t5分钟内\t数据最小时间\t\t数据最大时间\t\t仪器数\t位置厂家\n");
                    iii = 99;
                    snprintf(Lbuf, sizeof(Lbuf), "%03d\t", ++itm);
                    for (int i = 0; i < nl; i++) {
                        // 空行 ？ 非空时 ：空行时
                        snprintf((char *)&Lbuf[strlen(Lbuf)], sizeof(Lbuf) - strlen(Lbuf), "%6s\t", row[i] ? row[i] : "NULL");
                        switch (i) {
                            case 1: sscanf(row[i], "%d", &r1); Trec += r1; break;
                            case 2: sscanf(row[i], "%d", &r2); Trec += r1; break;
                            case 5: sscanf(row[i], "%d", &r5); break;
                        }
                    }
                    snprintf((char *)&Lbuf[strlen(Lbuf)], sizeof(Lbuf) - strlen(Lbuf), "%s\t", mSTA[wik].sv);
                    if (r2 == r5 && r5 > 20) {
                        snprintf((char *)&Lbuf[strlen(Lbuf)], sizeof(Lbuf) - strlen(Lbuf), "(%02d)", ++isN);
                        printf("\e[92m%s ", Lbuf);
                        sqlProST(conn, row[0], row[3]);
                    } else {
                        printf("\e[37m");
                        printf("%s\n", Lbuf);
                    }

                    printf("\e[37m");

                    mSTA[wik].ik = -1; // 标记：接收到该站的数据
                }
            }
            mysql_free_result(result);
        }
    }
    printf("\e[37m");
    mysql_close(conn); // 关闭连接

    if (Trec > 0) printf("写入数据库数据包总数：%d\n", Trec);
    int i, j;
    for (j = i = 0; i < STAn; i++) {
        if (mSTA[i].ik > -1) // ==-1 接收过数据
        {
            static char iii = 0;
            if (iii == 0) printf("\n-------------没有数据入库的台站：\n");
            iii = 99;
            printf("(%3d)  %s %s\t", ++j, mSTA[i].id, mSTA[i].sv);
            if (j % 5 == 0) printf("\n");
        }
    }
    printf("\n");
    return 0;
}
